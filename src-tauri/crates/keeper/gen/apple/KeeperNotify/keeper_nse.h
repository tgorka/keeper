// The C ABI of `src-tauri/crates/keeper-nse` (`libkeeper_nse.a`), for
// NotificationService.swift. `keeper_nse_handle` takes a NUL-terminated JSON
// request valid for the call and returns a JSON answer the caller releases
// exactly once with `keeper_nse_free`.
#ifndef KEEPER_NSE_H
#define KEEPER_NSE_H

char *keeper_nse_handle(const char *request);
void keeper_nse_free(char *answer);

#endif
