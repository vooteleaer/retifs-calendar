/*
 * retifs Calendar for Haiku, with the Haiku API: the same calendar as on
 * Linux. A month to pick a day from (days with something on are marked), the
 * calendars (libraries) to show and the contacts' birthdays, the day's events
 * and what's coming up in two weeks; an editor for an event (when, where,
 * repeating, who), and sharing a calendar or one event.
 *
 * Everything comes from the shared Rust core through retifs_calendar.h, as
 * JSON, read into BMessages with BJson. The core's calls block, so they run
 * in a looper of their own (Worker), which answers whoever asked.
 */

#include <Alert.h>
#include <Application.h>
#include <Button.h>
#include <CheckBox.h>
#include <Clipboard.h>
#include <DateTime.h>
#include <GroupView.h>
#include <LayoutBuilder.h>
#include <ListItem.h>
#include <ListView.h>
#include <MenuField.h>
#include <MenuItem.h>
#include <MessageRunner.h>
#include <PopUpMenu.h>
#include <ScrollView.h>
#include <SeparatorView.h>
#include <StringList.h>
#include <StringView.h>
#include <TextControl.h>
#include <TextView.h>
#include <Window.h>

#include <CalendarView.h>
#include <Json.h>

#include <set>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vector>

#include "retifs_calendar.h"

using BPrivate::BCalendarView;

static const char* kSignature = "application/x-vnd.retifs-calendar";
static const char* kMe = "me";

enum {
	kCall = 'call',			// worker: run "op"; answer "reply_to" with kAnswer
	kAnswer = 'answ',
	kDaySelected = 'dsel',
	kPrevMonth = 'prev',
	kNextMonth = 'next',
	kToday = 'tody',
	kShown = 'shwn',		// a calendar or birthdays switched on or off
	kCheck = 'chck',		// time to see whether the libraries changed
	kNewEvent = 'newe',
	kOpenDay = 'opnd',
	kOpenComing = 'opnc',
	kShareCalendar = 'shrc',
	kChanged = 'chgd',		// an editor saved or deleted: reload
	kAllDay = 'alld',
	kSave = 'save',
	kDelete = 'dele',
	kShareEvent = 'shre',
	kCancel = 'cncl',
	kMember = 'mmbr',
	kReader = 'rder',
	kBusy = 'busy',
	kCopy = 'copy',
	kClose = 'clos',
};

static const char* kMonths[] = { "January", "February", "March", "April",
	"May", "June", "July", "August", "September", "October", "November",
	"December" };
static const char* kWeekdays[] = { "Monday", "Tuesday", "Wednesday",
	"Thursday", "Friday", "Saturday", "Sunday" };


// #pragma mark - JSON helpers


/*! Parses a string the core returned (and frees it), keeping the text too. */
static BMessage
Take(char* json, BString* raw = NULL)
{
	BMessage m;
	if (json != NULL) {
		BJson::Parse(json, m);
		if (raw != NULL)
			raw->SetTo(json);
		rc_free(json);
	}
	return m;
}


/*! A JSON array as BJson gives it: fields named "0", "1", ... */
static int32
Count(const BMessage& array)
{
	return array.HasString("error") ? 0 : array.CountNames(B_ANY_TYPE);
}


static BMessage
Item(const BMessage& array, int32 i)
{
	BMessage item;
	BString name;
	name << i;
	array.FindMessage(name.String(), &item);
	return item;
}


static BString
ItemString(const BMessage& array, int32 i)
{
	BString name;
	name << i;
	return array.GetString(name.String(), "");
}


static BString
JsonString(const char* s)
{
	BString out("\"");
	for (const char* p = s; *p; p++) {
		switch (*p) {
			case '"': out << "\\\""; break;
			case '\\': out << "\\\\"; break;
			case '\n': out << "\\n"; break;
			case '\t': out << "\\t"; break;
			default: out << *p;
		}
	}
	out << "\"";
	return out;
}


static BString
JsonArray(const BStringList& list)
{
	BString out("[");
	for (int32 i = 0; i < list.CountStrings(); i++) {
		if (i > 0)
			out << ",";
		out << JsonString(list.StringAt(i).String());
	}
	out << "]";
	return out;
}


static BString
DateString(const BDate& d)
{
	BString s;
	s.SetToFormat("%04" B_PRId32 "-%02" B_PRId32 "-%02" B_PRId32, d.Year(),
		d.Month(), d.Day());
	return s;
}


static BDate
ParseDate(const char* s)
{
	int y, m, d;
	if (sscanf(s, "%d-%d-%d", &y, &m, &d) == 3)
		return BDate(y, m, d);
	return BDate::CurrentDate(B_LOCAL_TIME);
}


static BString
CalendarName(const BString& ns)
{
	return ns == kMe ? BString("Mine (me)") : ns;
}


// #pragma mark - Worker


/*! Runs the core's calls one after another, off the windows' threads. */
class Worker : public BLooper {
public:
	Worker() : BLooper("calendar worker") {}

	void MessageReceived(BMessage* message)
	{
		if (message->what != kCall) {
			BLooper::MessageReceived(message);
			return;
		}
		BString op = message->GetString("op", "");
		const char* a = message->GetString("a", "");
		const char* b = message->GetString("b", "");
		const char* c = message->GetString("c", "");
		int32 n = message->GetInt32("n", 0);
		BString raw;
		BMessage result;
		if (op == "libraries")
			result = Take(rc_libraries());
		else if (op == "load")
			result = Take(rc_occurrences(a, b, c, n));
		else if (op == "counts")
			result = Take(rc_change_counts(a), &raw);
		else if (op == "people")
			result = Take(rc_people(a));
		else if (op == "form")
			result = Take(rc_form_of(a, b));
		else if (op == "repeats")
			result = Take(rc_repeats());
		else if (op == "save")
			result = Take(rc_save(a, b));
		else if (op == "delete")
			result = Take(rc_delete(a));
		else if (op == "invite")
			result = Take(rc_invite(a));
		else if (op == "share")
			result = Take(rc_share_calendar(a, n));
		else if (op == "share_event")
			result = Take(rc_share_event(a));

		BMessage answer(kAnswer);
		answer.AddString("op", op);
		answer.AddMessage("result", &result);
		answer.AddString("raw", raw);
		// What the asker needs to know which answer this is.
		BMessage context;
		if (message->FindMessage("context", &context) == B_OK)
			answer.AddMessage("context", &context);
		BMessenger replyTo;
		if (message->FindMessenger("reply_to", &replyTo) == B_OK)
			replyTo.SendMessage(&answer);
	}
};


static Worker* sWorker = NULL;


static void
Call(BHandler* replyTo, const char* op, const char* a = "", const char* b = "",
	const char* c = "", int32 n = 0, BMessage* context = NULL)
{
	BMessage call(kCall);
	call.AddString("op", op);
	call.AddString("a", a);
	call.AddString("b", b);
	call.AddString("c", c);
	call.AddInt32("n", n);
	call.AddMessenger("reply_to", BMessenger(replyTo));
	if (context != NULL)
		call.AddMessage("context", context);
	sWorker->PostMessage(&call);
}


// #pragma mark - LinkWindow


/*! A link to give someone, to copy. */
class LinkWindow : public BWindow {
public:
	LinkWindow(const char* heading, const char* body, const BMessage& result)
		:
		BWindow(BRect(200, 200, 700, 420), heading, B_FLOATING_WINDOW_LOOK,
			B_NORMAL_WINDOW_FEEL, B_AUTO_UPDATE_SIZE_LIMITS | B_CLOSE_ON_ESCAPE)
	{
		fLink = result.GetString("link", "");
		BString error = result.GetString("error", "");

		BTextView* text = new BTextView("body");
		text->MakeEditable(false);
		text->SetText(error.IsEmpty() ? body : error.String());
		text->SetExplicitMinSize(BSize(440, 50));
		text->SetViewUIColor(B_PANEL_BACKGROUND_COLOR);

		BTextView* link = new BTextView("link");
		link->MakeEditable(false);
		link->SetText(fLink.String());
		link->SetExplicitMinSize(BSize(440, 60));

		BButton* copy = new BButton("Copy link", new BMessage(kCopy));
		copy->SetEnabled(!fLink.IsEmpty());
		BLayoutBuilder::Group<>(this, B_VERTICAL)
			.SetInsets(B_USE_WINDOW_SPACING)
			.Add(text)
			.Add(new BScrollView("ls", link, 0, false, false))
			.AddGroup(B_HORIZONTAL)
				.AddGlue()
				.Add(copy)
				.Add(new BButton("Close", new BMessage(kClose)))
			.End();
		CenterOnScreen();
	}

	void MessageReceived(BMessage* message)
	{
		switch (message->what) {
			case kCopy:
				if (be_clipboard->Lock()) {
					be_clipboard->Clear();
					be_clipboard->Data()->AddData("text/plain", B_MIME_TYPE,
						fLink.String(), fLink.Length());
					be_clipboard->Commit();
					be_clipboard->Unlock();
				}
				break;
			case kClose:
				Quit();
				break;
			default:
				BWindow::MessageReceived(message);
		}
	}

private:
	BString fLink;
};


// #pragma mark - EventWindow


/*! The editor for an event (new, or one saved before). */
class EventWindow : public BWindow {
public:
	EventWindow(BMessenger main, const BString& eventJson, bool isNew,
		const BStringList& libraries, const BDate& day)
		:
		BWindow(BRect(150, 120, 650, 720), isNew ? "New event" : "Event",
			B_TITLED_WINDOW, B_AUTO_UPDATE_SIZE_LIMITS | B_CLOSE_ON_ESCAPE),
		fMain(main),
		fEvent(eventJson),
		fNew(isNew),
		fLibraries(libraries)
	{
		fTitle = new BTextControl("Title", "", NULL);
		BPopUpMenu* libs = new BPopUpMenu("calendar");
		for (int32 i = 0; i < libraries.CountStrings(); i++)
			libs->AddItem(new BMenuItem(CalendarName(libraries.StringAt(i)), NULL));
		if (libs->ItemAt(0) != NULL)
			libs->ItemAt(0)->SetMarked(true);
		fLibrary = new BMenuField("Calendar", libs);
		fLibrary->SetEnabled(isNew);
		fDate = new BTextControl("Date (YYYY-MM-DD)", DateString(day), NULL);
		fAllDay = new BCheckBox("All day", new BMessage(kAllDay));
		fFrom = new BTextControl("From (HH:MM)", "12:00", NULL);
		fUntil = new BTextControl("Until (HH:MM)", "13:00", NULL);
		fRepeats = new BMenuField("Repeats", new BPopUpMenu("repeats"));
		fLocation = new BTextControl("Location", "", NULL);
		fNote = new BTextControl("Note", "", NULL);
		fWho = new BListView("who", B_MULTIPLE_SELECTION_LIST);

		BButton* deleteButton = new BButton("Delete", new BMessage(kDelete));
		BButton* share = new BButton("Share" B_UTF8_ELLIPSIS,
			new BMessage(kShareEvent));
		deleteButton->SetEnabled(!isNew);
		share->SetEnabled(!isNew);
		BButton* save = new BButton("Save", new BMessage(kSave));

		BLayoutBuilder::Group<>(this, B_VERTICAL)
			.SetInsets(B_USE_WINDOW_SPACING)
			.AddGrid()
				.AddTextControl(fTitle, 0, 0)
				.AddMenuField(fLibrary, 0, 1)
				.AddTextControl(fDate, 0, 2)
				.Add(fAllDay, 1, 3)
				.AddTextControl(fFrom, 0, 4)
				.AddTextControl(fUntil, 0, 5)
				.AddMenuField(fRepeats, 0, 6)
				.AddTextControl(fLocation, 0, 7)
				.AddTextControl(fNote, 0, 8)
			.End()
			.Add(new BStringView("w", "Who: your contacts (select to invite; "
				"their cards list the events they're at)"))
			.Add(new BScrollView("ws", fWho, 0, false, true))
			.AddGroup(B_HORIZONTAL)
				.Add(deleteButton)
				.Add(share)
				.AddGlue()
				.Add(new BButton("Cancel", new BMessage(kCancel)))
				.Add(save)
			.End();
		SetDefaultButton(save);
		CenterOnScreen();

		Call(this, "repeats");
		Call(this, "form", fEvent.String(), DateString(day).String());
		Call(this, "people", JsonArray(libraries).String());
	}

	void MessageReceived(BMessage* message)
	{
		switch (message->what) {
			case kAnswer:
				_Answer(message);
				break;
			case kAllDay:
				fFrom->SetEnabled(fAllDay->Value() != B_CONTROL_ON);
				fUntil->SetEnabled(fAllDay->Value() != B_CONTROL_ON);
				break;
			case kSave:
				Call(this, "save", fEvent.String(), _FormJson().String());
				break;
			case kDelete:
				Call(this, "delete", fEvent.String());
				break;
			case kShareEvent:
				Call(this, "share_event", fEvent.String());
				break;
			case kCancel:
				Quit();
				break;
			default:
				BWindow::MessageReceived(message);
		}
	}

private:
	void _Answer(BMessage* message)
	{
		BString op = message->GetString("op", "");
		BMessage r;
		message->FindMessage("result", &r);
		BString error = r.GetString("error", "");
		if (op == "repeats") {
			BMenu* menu = fRepeats->Menu();
			for (int32 i = 0; i < Count(r); i++)
				menu->AddItem(new BMenuItem(ItemString(r, i), NULL));
			_MarkRepeat();
		} else if (op == "form") {
			fForm = r;
			fTitle->SetText(r.GetString("title", ""));
			fDate->SetText(r.GetString("date", ""));
			fAllDay->SetValue(r.GetBool("all_day", false) ? B_CONTROL_ON
				: B_CONTROL_OFF);
			fFrom->SetText(r.GetString("from", ""));
			fUntil->SetText(r.GetString("until", ""));
			fLocation->SetText(r.GetString("location", ""));
			fNote->SetText(r.GetString("note", ""));
			PostMessage(kAllDay);
			_MarkRepeat();
			_MarkAttendees();
		} else if (op == "people") {
			fWho->MakeEmpty();
			fPeopleIds.MakeEmpty();
			fPeopleNames.MakeEmpty();
			for (int32 i = 0; i < Count(r); i++) {
				BMessage p = Item(r, i);
				BString label = p.GetString("name", "");
				BString ns = p.GetString("ns", "");
				if (ns != kMe)
					label << "  (" << ns << ")";
				fWho->AddItem(new BStringItem(label));
				fPeopleIds.Add(p.GetString("id", ""));
				fPeopleNames.Add(p.GetString("name", ""));
			}
			_MarkAttendees();
		} else if (op == "save" || op == "delete") {
			if (!error.IsEmpty()) {
				(new BAlert("Calendar", error.String(), "OK", NULL, NULL,
					B_WIDTH_AS_USUAL, B_WARNING_ALERT))->Go(NULL);
				return;
			}
			BMessage changed(kChanged);
			changed.AddString("date", r.GetString("date", ""));
			fMain.SendMessage(&changed);
			Quit();
		} else if (op == "share_event") {
			(new LinkWindow("Share this event", "Whoever opens this link sees "
				"this event, and sees it when it changes. Nothing else of the "
				"calendar.", r))->Show();
		}
	}

	void _MarkRepeat()
	{
		BMenuItem* item = fRepeats->Menu()->ItemAt(
			fForm.GetDouble("repeat", 0));
		if (item != NULL)
			item->SetMarked(true);
		else if (fRepeats->Menu()->ItemAt(0) != NULL)
			fRepeats->Menu()->ItemAt(0)->SetMarked(true);
	}

	void _MarkAttendees()
	{
		BMessage attendees;
		fForm.FindMessage("attendees", &attendees);
		for (int32 i = 0; i < fPeopleIds.CountStrings(); i++) {
			for (int32 j = 0; j < Count(attendees); j++) {
				if (ItemString(attendees, j) == fPeopleIds.StringAt(i))
					fWho->Select(i, true);
			}
		}
	}

	BString _FormJson()
	{
		BString json("{");
		json << "\"title\":" << JsonString(fTitle->Text())
			<< ",\"date\":" << JsonString(fDate->Text())
			<< ",\"all_day\":" << (fAllDay->Value() == B_CONTROL_ON ? "true" : "false")
			<< ",\"from\":" << JsonString(fFrom->Text())
			<< ",\"until\":" << JsonString(fUntil->Text())
			<< ",\"repeat\":" << fRepeats->Menu()->IndexOf(
				fRepeats->Menu()->FindMarked())
			<< ",\"location\":" << JsonString(fLocation->Text())
			<< ",\"note\":" << JsonString(fNote->Text());
		int32 lib = fLibrary->Menu()->IndexOf(fLibrary->Menu()->FindMarked());
		if (fNew && lib >= 0)
			json << ",\"ns\":" << JsonString(fLibraries.StringAt(lib).String());
		json << ",\"attendees\":[";
		bool first = true;
		for (int32 i = 0; i < fPeopleIds.CountStrings(); i++) {
			if (!fWho->IsItemSelected(i))
				continue;
			if (!first)
				json << ",";
			first = false;
			json << "[" << JsonString(fPeopleIds.StringAt(i).String()) << ","
				<< JsonString(fPeopleNames.StringAt(i).String()) << "]";
		}
		json << "]}";
		return json;
	}

	BMessenger fMain;
	BString fEvent;
	bool fNew;
	BStringList fLibraries;
	BMessage fForm;
	BStringList fPeopleIds;
	BStringList fPeopleNames;
	BTextControl* fTitle;
	BMenuField* fLibrary;
	BTextControl* fDate;
	BCheckBox* fAllDay;
	BTextControl* fFrom;
	BTextControl* fUntil;
	BMenuField* fRepeats;
	BTextControl* fLocation;
	BTextControl* fNote;
	BListView* fWho;
};


// #pragma mark - ShareWindow


/*! Sharing a calendar: with a member who writes to it too, or for reading,
	all of it or only when it's busy. */
class ShareWindow : public BWindow {
public:
	ShareWindow(const BStringList& libraries)
		:
		BWindow(BRect(200, 200, 640, 360), "Share a calendar", B_TITLED_WINDOW,
			B_AUTO_UPDATE_SIZE_LIMITS | B_CLOSE_ON_ESCAPE),
		fLibraries(libraries)
	{
		BPopUpMenu* menu = new BPopUpMenu("calendar");
		for (int32 i = 0; i < libraries.CountStrings(); i++)
			menu->AddItem(new BMenuItem(libraries.StringAt(i), NULL));
		if (menu->ItemAt(0) != NULL)
			menu->ItemAt(0)->SetMarked(true);
		fPick = new BMenuField("Calendar", menu);

		BTextView* text = new BTextView("about");
		text->MakeEditable(false);
		text->SetViewUIColor(B_PANEL_BACKGROUND_COLOR);
		text->SetText("A member reads and writes it (a colleague). A reader "
			"sees all of it. Free/busy shows only when it's busy: no titles, "
			"places, notes or people.");
		text->SetExplicitMinSize(BSize(400, 50));

		BLayoutBuilder::Group<>(this, B_VERTICAL)
			.SetInsets(B_USE_WINDOW_SPACING)
			.Add(text)
			.Add(fPick)
			.AddGroup(B_HORIZONTAL)
				.AddGlue()
				.Add(new BButton("Free/busy", new BMessage(kBusy)))
				.Add(new BButton("Reader", new BMessage(kReader)))
				.Add(new BButton("Member", new BMessage(kMember)))
			.End();
		CenterOnScreen();
	}

	void MessageReceived(BMessage* message)
	{
		int32 i = fPick->Menu()->IndexOf(fPick->Menu()->FindMarked());
		BString ns = i >= 0 ? fLibraries.StringAt(i) : BString();
		switch (message->what) {
			case kMember:
				Call(this, "invite", ns.String());
				break;
			case kReader:
				Call(this, "share", ns.String(), "", "", 0);
				break;
			case kBusy:
				Call(this, "share", ns.String(), "", "", 1);
				break;
			case kAnswer:
			{
				BString op = message->GetString("op", "");
				BMessage r;
				message->FindMessage("result", &r);
				if (op == "invite")
					(new LinkWindow("Invite a member", "Whoever opens this "
						"invitation first joins the calendar and may write to "
						"it. It works once, for a week.", r))->Show();
				else
					(new LinkWindow("Share the calendar", "Whoever has this "
						"link reads the calendar (or, free/busy, sees when it's "
						"busy and nothing else).", r))->Show();
				break;
			}
			default:
				BWindow::MessageReceived(message);
		}
	}

private:
	BMenuField* fPick;
	BStringList fLibraries;
};


// #pragma mark - Month


/*! The month to pick a day from, with the days that have something on
	marked. */
class Month : public BCalendarView {
public:
	Month() : BCalendarView("month") {}

	void SetMarked(const std::set<int>& days)
	{
		fMarked = days;
		Invalidate();
	}

	void DrawDay(BView* owner, BRect frame, const char* text, bool isSelected,
		bool isEnabled, bool focus, bool highlight)
	{
		BCalendarView::DrawDay(owner, frame, text, isSelected, isEnabled,
			focus, highlight);
		if (isEnabled && fMarked.count(atoi(text)) > 0) {
			owner->SetHighColor(ui_color(B_CONTROL_HIGHLIGHT_COLOR));
			owner->FillEllipse(BPoint((frame.left + frame.right) / 2,
				frame.bottom - 3), 2, 2);
		}
	}

private:
	std::set<int> fMarked;
};


// #pragma mark - CalendarWindow


class CalendarWindow : public BWindow {
public:
	CalendarWindow()
		:
		BWindow(BRect(80, 80, 980, 700), "Calendar", B_TITLED_WINDOW,
			B_AUTO_UPDATE_SIZE_LIMITS | B_QUIT_ON_WINDOW_CLOSE)
	{
		fMonth = new Month();
		fMonth->SetSelectionMessage(new BMessage(kDaySelected));
		fMonth->SetInvocationMessage(new BMessage(kNewEvent));
		fMonth->SetTarget(this);
		fMonthLabel = new BStringView("monthLabel", "");
		BFont bold(be_bold_font);
		fMonthLabel->SetFont(&bold);
		fCalendars = new BGroupView(B_VERTICAL, 0);
		fBirthdays = new BCheckBox("Birthdays (from your contacts)",
			new BMessage(kShown));
		fBirthdays->SetValue(B_CONTROL_ON);

		fDayTitle = new BStringView("dayTitle", "");
		BFont big(be_bold_font);
		big.SetSize(big.Size() * 1.3);
		fDayTitle->SetFont(&big);
		fDayLabel = new BStringView("dayLabel", "This day");
		fDayList = new BListView("day");
		fDayList->SetInvocationMessage(new BMessage(kOpenDay));
		fComing = new BListView("coming");
		fComing->SetInvocationMessage(new BMessage(kOpenComing));

		BLayoutBuilder::Group<>(this, B_HORIZONTAL)
			.SetInsets(B_USE_WINDOW_SPACING)
			.AddGroup(B_VERTICAL)
				.AddGroup(B_HORIZONTAL)
					.Add(new BButton("◀", new BMessage(kPrevMonth)))
					.AddGlue()
					.Add(fMonthLabel)
					.AddGlue()
					.Add(new BButton("▶", new BMessage(kNextMonth)))
				.End()
				.Add(fMonth)
				.Add(new BButton("Today", new BMessage(kToday)))
				.Add(new BSeparatorView(B_HORIZONTAL))
				.Add(new BStringView("c", "Calendars"))
				.Add(fCalendars)
				.Add(fBirthdays)
				.AddGlue()
			.End()
			.Add(new BSeparatorView(B_VERTICAL))
			.AddGroup(B_VERTICAL)
				.AddGroup(B_HORIZONTAL)
					.Add(fDayTitle)
					.AddGlue()
					.Add(new BButton("Share calendar" B_UTF8_ELLIPSIS,
						new BMessage(kShareCalendar)))
					.Add(new BButton("New event", new BMessage(kNewEvent)))
				.End()
				.Add(fDayLabel)
				.Add(new BScrollView("ds", fDayList, 0, false, true), 2)
				.Add(new BStringView("cu", "Coming up: the next two weeks"))
				.Add(new BScrollView("cs", fComing, 0, false, true), 3)
			.End();

		_UpdateTitles();
		Call(this, "libraries");
		fRunner = new BMessageRunner(BMessenger(this), BMessage(kCheck), 3000000);
	}

	~CalendarWindow()
	{
		delete fRunner;
	}

	void MessageReceived(BMessage* message)
	{
		switch (message->what) {
			case kAnswer:
				_Answer(message);
				break;
			case kDaySelected:
			case kShown:
				_Reload();
				break;
			case kPrevMonth:
			case kNextMonth:
			{
				BDate d = fMonth->Date();
				int32 delta = message->what == kNextMonth ? 1 : -1;
				int32 y = d.Year(), m = d.Month() + delta;
				if (m < 1) { m = 12; y--; }
				if (m > 12) { m = 1; y++; }
				BDate first(y, m, 1);
				int32 day = d.Day() < first.DaysInMonth() ? d.Day()
					: first.DaysInMonth();
				fMonth->SetDate(y, m, day);
				_Reload();
				break;
			}
			case kToday:
				fMonth->SetDate(BDate::CurrentDate(B_LOCAL_TIME));
				_Reload();
				break;
			case kCheck:
				Call(this, "counts", JsonArray(_Shown()).String());
				break;
			case kNewEvent:
			{
				BStringList shown = _Shown();
				BString ns = shown.CountStrings() > 0 ? shown.StringAt(0)
					: BString(kMe);
				char* event = rc_new_event(ns.String(),
					DateString(fMonth->Date()).String());
				BString json(event);
				rc_free(event);
				(new EventWindow(BMessenger(this), json, true, fLibraries,
					fMonth->Date()))->Show();
				break;
			}
			case kOpenDay:
			case kOpenComing:
			{
				bool day = message->what == kOpenDay;
				BListView* list = day ? fDayList : fComing;
				std::vector<int32>& rows = day ? fDayRows : fComingRows;
				int32 i = list->CurrentSelection();
				if (i < 0 || i >= (int32)rows.size())
					break;
				BMessage o = Item(fOccurrences, rows[i]);
				if (o.GetBool("birthday", false))
					break;
				(new EventWindow(BMessenger(this), o.GetString("event_json", ""),
					false, fLibraries, fMonth->Date()))->Show();
				break;
			}
			case kShareCalendar:
			{
				BStringList others;
				for (int32 i = 0; i < fLibraries.CountStrings(); i++) {
					if (fLibraries.StringAt(i) != kMe)
						others.Add(fLibraries.StringAt(i));
				}
				if (others.IsEmpty()) {
					(new BAlert("Calendar", "Your own calendar (me) is yours "
						"alone. Make a calendar to share first.", "OK"))->Go(NULL);
					break;
				}
				(new ShareWindow(others))->Show();
				break;
			}
			case kChanged:
			{
				BString date = message->GetString("date", "");
				if (!date.IsEmpty())
					fMonth->SetDate(ParseDate(date.String()));
				_Reload();
				break;
			}
			default:
				BWindow::MessageReceived(message);
		}
	}

private:
	void _Answer(BMessage* message)
	{
		BString op = message->GetString("op", "");
		BMessage r;
		message->FindMessage("result", &r);
		if (op == "libraries") {
			fLibraries.MakeEmpty();
			for (int32 i = 0; i < Count(r); i++)
				fLibraries.Add(ItemString(r, i));
			if (fLibraries.IsEmpty())
				fLibraries.Add(kMe);
			for (int32 i = 0; i < fLibraries.CountStrings(); i++) {
				BCheckBox* box = new BCheckBox(fLibraries.StringAt(i).String(),
					CalendarName(fLibraries.StringAt(i)), new BMessage(kShown));
				box->SetValue(B_CONTROL_ON);
				fCalendars->AddChild(box);
				box->SetTarget(this);
			}
			_Reload();
		} else if (op == "load") {
			BMessage context;
			message->FindMessage("context", &context);
			// Only the answer to the latest question counts.
			if (context.GetInt32("generation", 0) != fGeneration)
				return;
			fOccurrences = r;
			_ShowDay();
		} else if (op == "counts") {
			BString raw = message->GetString("raw", "");
			if (!fCounts.IsEmpty() && raw != fCounts)
				_Reload();
			fCounts = raw;
		}
	}

	BStringList _Shown()
	{
		BStringList shown;
		for (int32 i = 0; i < fCalendars->CountChildren(); i++) {
			BCheckBox* box = dynamic_cast<BCheckBox*>(fCalendars->ChildAt(i));
			if (box != NULL && box->Value() == B_CONTROL_ON)
				shown.Add(box->Name());
		}
		return shown;
	}

	void _UpdateTitles()
	{
		BDate d = fMonth->Date();
		BString month;
		month << kMonths[d.Month() - 1] << " " << d.Year();
		fMonthLabel->SetText(month);
		BString title;
		title << kWeekdays[d.DayOfWeek() - 1] << " " << d.Day() << " "
			<< kMonths[d.Month() - 1] << " " << d.Year();
		fDayTitle->SetText(title);
	}

	/*! The shown month (for its marks), the day, and two weeks on. */
	void _Reload()
	{
		_UpdateTitles();
		BDate day = fMonth->Date();
		BDate first(day.Year(), day.Month(), 1);
		BDate until = first;
		until.AddDays(45);
		BDate twoWeeks = day;
		twoWeeks.AddDays(15);
		if (twoWeeks > until)
			until = twoWeeks;
		BMessage context;
		context.AddInt32("generation", ++fGeneration);
		Call(this, "load", JsonArray(_Shown()).String(),
			DateString(first).String(), DateString(until).String(),
			fBirthdays->Value() == B_CONTROL_ON, &context);
	}

	void _ShowDay()
	{
		BDate d = fMonth->Date();
		BString day = DateString(d);
		BDate end = d;
		end.AddDays(14);
		BString soon = DateString(end);
		BString monthPrefix(day.String(), 8);	// YYYY-MM-

		std::set<int> marked;
		for (BListItem* item; (item = fDayList->RemoveItem((int32)0)) != NULL;)
			delete item;
		for (BListItem* item; (item = fComing->RemoveItem((int32)0)) != NULL;)
			delete item;
		fDayRows.clear();
		fComingRows.clear();
		for (int32 i = 0; i < Count(fOccurrences); i++) {
			BMessage o = Item(fOccurrences, i);
			BString first = o.GetString("first", "");
			BString until = o.GetString("until", "");
			if (first.StartsWith(monthPrefix))
				marked.insert(atoi(first.String() + 8));
			if (first <= day && day < until) {
				fDayList->AddItem(new BStringItem(_Line(o, false)));
				fDayRows.push_back(i);
			} else if (first > day && first <= soon) {
				fComing->AddItem(new BStringItem(_Line(o, true)));
				fComingRows.push_back(i);
			}
		}
		fDayLabel->SetText(fDayRows.empty() ? "Nothing on this day"
			: "This day");
		fMonth->SetMarked(marked);
	}

	BString _Line(const BMessage& o, bool withDate)
	{
		BString line;
		if (o.GetBool("birthday", false))
			line << "★ ";
		if (withDate)
			line << o.GetString("weekday", "") << " · ";
		line << o.GetString("time", "") << "   " << o.GetString("title", "");
		BString extra;
		const char* location = o.GetString("location", "");
		const char* people = o.GetString("people", "");
		BString ns = o.GetString("ns", "");
		if (*location)
			extra << " · " << location;
		if (*people)
			extra << " · " << people;
		if (ns != kMe)
			extra << " · " << ns;
		if (o.GetBool("repeats", false))
			extra << " · repeats";
		line << extra;
		return line;
	}

	Month* fMonth;
	BStringView* fMonthLabel;
	BGroupView* fCalendars;
	BCheckBox* fBirthdays;
	BStringView* fDayTitle;
	BStringView* fDayLabel;
	BListView* fDayList;
	BListView* fComing;
	std::vector<int32> fDayRows;
	std::vector<int32> fComingRows;
	BStringList fLibraries;
	BMessage fOccurrences;
	BString fCounts;
	int32 fGeneration = 0;
	BMessageRunner* fRunner;
};


// #pragma mark - App


class CalendarApp : public BApplication {
public:
	CalendarApp() : BApplication(kSignature) {}

	void ReadyToRun()
	{
		sWorker = new Worker();
		sWorker->Run();
		(new CalendarWindow())->Show();
	}

	bool QuitRequested()
	{
		if (sWorker != NULL && sWorker->Lock())
			sWorker->Quit();
		sWorker = NULL;
		return true;
	}
};


int
main()
{
	CalendarApp app;
	app.Run();
	return 0;
}
