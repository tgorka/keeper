# Step 2: Write the cards

1. For each piece of work, write one card into this session with `session_write`: tags `[task]`, a
   title, `assignee` (the agent of this drive whose work it is), `requested_by` (who asked) and a
   body that says what done looks like.
2. Write `artifacts/triage-<today>.md` listing each card: title, assignee, who asked; then what you
   left out and why.
3. Ask the person, with `ask_human`, whether to hand the cards on now: choices `Hand on` and
   `Hold`, default `Hold`. Then end your turn.
4. When the answer comes: `Hand on` — start the `dispatch` workflow with `workflow_start`, naming
   this session as its `session` input; `Hold` — leave the cards. Then `reply` with the list.
