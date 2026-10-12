// The app target's bridging header (Epic 98, story 98.1): the three C-ABI
// functions `src-tauri/crates/keeper/src/push_ios.rs` exports from libapp.a
// for KeeperPush.swift. `keeper_push_action` returns null or a JSON string
// the caller releases exactly once with `keeper_push_free`; every argument is
// a NUL-terminated buffer valid for the call.
#ifndef KEEPER_PUSH_H
#define KEEPER_PUSH_H

char *keeper_push_action(const char *request);
void keeper_push_open(const char *user_info);
void keeper_push_free(char *text);

#endif
