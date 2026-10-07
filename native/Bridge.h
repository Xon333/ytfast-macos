#ifndef YTFAST_BRIDGE_H
#define YTFAST_BRIDGE_H
// All calls run on the application main thread. Returned strings are owned by
// Rust and must be released once with ytfast_free. wake is nonblocking.
char *ytfast_start(void (*wake)(void));
char *ytfast_call(const char *request);
void ytfast_free(char *response);
void ytfast_stop(void);
#endif
