# Step 1: Hand the cards on

1. Read the cards of the session the `session` input names with your drive tools.
2. For each card that has an `assignee` and no `run`, call `delegate`: the card's body as the
   brief, its title as the new card's title, only the drives the work needs, and as `source` the
   session's id, a colon and the card's file name (`<session>:<card>`). A card handed on before —
   by this run, another dispatch or the session itself — is not handed on again: the call answers
   with its delegation.
3. `reply` with what you handed on, to whom, what was handed on already, and anything you held
   back because a bound or a label stopped it.
