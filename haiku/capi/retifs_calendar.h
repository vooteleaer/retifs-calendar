/*
 * retifs Calendar's shared core, as C (haiku/capi): JSON in and out.
 * Every returned string is the caller's, freed with rc_free(). Calls block.
 * An event travels as the JSON string the core gave ("event_json").
 * Answers that can fail are objects with "error" when they did.
 */
#ifndef RETIFS_CALENDAR_H
#define RETIFS_CALENDAR_H

#ifdef __cplusplus
extern "C" {
#endif

/* ["me", "family", ...] */
char* rc_libraries(void);
/* [{"title", "time", "first", "until", "weekday", "birthday", "repeats",
 *   "location", "people", "ns", "event_json"}]; dates YYYY-MM-DD, until exclusive */
char* rc_occurrences(const char* libraries_json, const char* from, const char* to,
	int birthdays);
/* [{"ns", "id", "name"}] */
char* rc_people(const char* libraries_json);
/* ["Never", "Every day", ...] */
char* rc_repeats(void);

/* A new event in ns on day (YYYY-MM-DD): its event JSON. */
char* rc_new_event(const char* ns, const char* day);
/* {"title", "date", "all_day", "from", "until", "repeat", "location", "note",
 *  "attendees": [id]} */
char* rc_form_of(const char* event_json, const char* day);
/* form as rc_form_of gives it, plus "ns" (new events) and
 * "attendees": [[id, name]]: {"event_json", "date"} or {"error"} */
char* rc_save(const char* event_json, const char* form_json);
char* rc_delete(const char* event_json);	/* {} or {"error"} */

/* {"link"} or {"error"} */
char* rc_invite(const char* ns);
char* rc_share_calendar(const char* ns, int free_busy);
char* rc_share_event(const char* event_json);

/* {ns: count}: reload when these differ */
char* rc_change_counts(const char* libraries_json);

void rc_free(char* s);

#ifdef __cplusplus
}
#endif

#endif
