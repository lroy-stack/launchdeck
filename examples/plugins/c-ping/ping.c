/* A LaunchDeck plugin in plain C, to show that the contract is the module
 * ABI and not a particular language. No libc, no allocator, no SDK: about
 * thirty lines. Build: ./build.sh (needs clang with the wasm32 target and
 * wasm-ld). */

#define IMPORT(name) __attribute__((import_module("launchdeck"), import_name(name)))
#define EXPORT(name) __attribute__((export_name(name)))

IMPORT("respond") void ld_respond(const char *ptr, int len);
IMPORT("publish") int ld_publish(const char *ptr, int len);
IMPORT("log") void ld_log(int level, const char *ptr, int len);

static char request[8192];

static int length(const char *s) { int n = 0; while (s[n]) n++; return n; }
static void respond(const char *json) { ld_respond(json, length(json)); }

EXPORT("ld_abi") int ld_abi(void) { return 1; }

/* The host writes the request here. One static buffer is enough because the
 * plugin finishes each request before the next one arrives. */
EXPORT("ld_alloc") char *ld_alloc(int len) { return len <= (int)sizeof request ? request : 0; }

/* Requests start {"op":"init"... or {"op":"invoke","action":"... so the byte
 * at offset 9 tells them apart, and the action id starts at offset 25. */
EXPORT("ld_handle") int ld_handle(const char *ptr, int len) {
  if (len < 26) { respond("{\"status\":\"failed\",\"error\":\"short request\"}"); return 0; }
  if (ptr[9] == 'i') { respond("{\"status\":\"ok\"}"); return 0; }
  if (ptr[25] == 'p') { /* ping.send */
    const char *event = "{\"type\":\"ping.sent\",\"payload\":{\"from\":\"c\"}}";
    ld_publish(event, length(event));
    ld_log(1, "pong", 4);
    respond("{\"status\":\"ok\",\"value\":{\"pong\":true}}");
    return 0;
  }
  respond("{\"status\":\"failed\",\"error\":\"unknown action\"}");
  return 0;
}
