//! Hermes' background-review prompts (`agent/background_review.py`), adapted
//! to keeper's tools: a review proposes through `memory_propose` and
//! `skill_propose`, reads skills through `skills_list` and `skill_view`, and
//! never edits a skill a person owns. Every sentence that does not name a
//! Hermes-only tool or rule is upstream's; `UPSTREAM.md` lists each change.
//!
//! Modified from `agent/background_review.py` of NousResearch/hermes-agent
//! (MIT, Copyright (c) 2025 Nous Research).

/// `_MEMORY_ROUTING_BLOCK`: which of the two files a fact belongs in.
macro_rules! memory_routing_block {
    () => {
        concat!(
            "TWO distinct stores — pick the right one for each fact:\n",
            "  • USER.md (memory_propose, target='user'): who the user is — persona, preferences, ",
            "communication and work style, personal details they revealed, and expectations about how you ",
            "should behave.\n",
            "  • MEMORY.md (memory_propose, target='memory'): facts about the ENVIRONMENT you operate in — ",
            "tool quirks, project conventions, config gotchas, paths and endpoints that matter.\n\n",
            "One fact goes to ONE store, never both — writing it to both bloats both files until they hit ",
            "their size limits and crowds out the facts that matter; misrouting it puts it where the next ",
            "session won't look.\n\n",
            "A proposal is staged, not saved: keeper applies it later, or a person does, and this session ",
            "keeps the memory it opened with.\n\n",
        )
    };
}

/// `_LESSON_LAYER_BLOCK`: what a skill is and how to write one.
macro_rules! lesson_layer_block {
    () => {
        concat!(
            "What a skill IS: the instructions for doing a class of task the most efficient and correct ",
            "way, to THIS user's specifications — the procedure, the tools and commands that work, the ",
            "order, the user's preferences for how the result should look, and the pitfalls that cost time. ",
            "A future session should be able to follow it and produce what the user wants on the first ",
            "try. Everything below is about writing that well:\n",
            "  • Procedure first: the steps in the order they are done, with the concrete commands, tool ",
            "calls, and decision points. Lessons and pitfalls attach to the step they affect.\n",
            "  • A pitfall is a generalizable rule + one clause of WHY (the mechanism), imperative. 'Grep the ",
            "test tree for the SYMBOL before widening a helper signature — hand-rolled mocks reimplement the ",
            "old shape and fail on a shard you did not run.' Not a narrative of what happened this session.\n",
            "  • No PR/issue numbers, dates, ticket IDs, or quoted user text as content — the rule must stand ",
            "without the incident behind it. Keep a short quote ONLY when the quote itself is the clearest ",
            "statement of the rule.\n",
            "  • The same lesson learned twice is ONE rule. Before adding, search the skill for the rule ",
            "already stated; strengthen or clarify it rather than appending a second copy.\n",
            "  • Not a duplicate of what the environment already teaches: repo AGENTS.md files, tool schema ",
            "descriptions, and other always-loaded context. A skill carries the WORKFLOW and the pitfalls; ",
            "it does not restate the codebase map or a tool's parameter list.\n",
            "  • Always-on rules (standing user preferences, gates that apply to every instance of the ",
            "task) live in SKILL.md itself, whole. keeper proposes SKILL.md only, so depth that is only ",
            "needed sometimes goes in a short topical section of it, never a '<date>-<incident>' section.\n",
            "  • Fix the skill in place when it is wrong: edit the sentence that misled, do not append ",
            "'UPDATE: actually...' underneath it.\n\n",
        )
    };
}

/// `_DO_NOT_CAPTURE_BLOCK`, verbatim: what never becomes a skill.
macro_rules! do_not_capture_block {
    () => {
        concat!(
            " (these become persistent self-imposed constraints that bite you later when the environment ",
            "changes):\n",
            "  • Environment-dependent failures: missing binaries, fresh-install errors, post-migration ",
            "path mismatches, 'command not found', unconfigured credentials, uninstalled packages. The ",
            "user can fix these — they are not durable rules.\n",
            "  • Negative claims about tools or features ('browser tools do not work', 'X tool is broken', ",
            "'cannot use Y from execute_code'). These harden into refusals the agent cites against itself ",
            "for months after the actual problem was fixed.\n",
            "  • Session-specific transient errors that resolved before the conversation ended. If ",
            "retrying worked, the lesson is the retry pattern, not the original failure.\n",
            "  • One-off task narratives. A user asking 'summarize today's market' or 'analyze this PR' is ",
            "not a class of work that warrants a skill.\n\n",
            "  • Unresolved failures: if the session ended WITHOUT actually finding a working method — you ",
            "tried several things, none worked, and told the user to check manually — do NOT write those ",
            "attempts up as a 'reliable workflow' or 'recommended approach'. That presents an untested ",
            "sequence of failures as validated guidance a future session will trust and repeat. Either say ",
            "'Nothing to save', or, only if you are independently confident of a real working alternative ",
            "(not something you are merely guessing might work), capture ONLY that alternative — never the ",
            "dead ends, and never dressed up as best practice.\n\n",
            "If a tool failed because of setup state, capture the FIX (install command, config step, env ",
            "var to set) under an existing setup or troubleshooting skill — never 'this tool does not ",
            "work' as a standalone constraint.\n\n",
        )
    };
}

/// Read-before-write and ownership, as keeper holds them (in place of
/// upstream's `skill_manage` guard and its protected-skill list).
macro_rules! keeper_skill_rules {
    () => {
        concat!(
            "Read-before-write: before you patch an existing skill, call skill_view(name) during this ",
            "review and write the whole new SKILL.md from what it just returned. Content quoted earlier ",
            "in the conversation transcript does NOT count. keeper pins a patch to the SKILL.md it was ",
            "proposed against, so a patch of a skill that changed since is dropped.\n\n",
            "Skills a person owns: a skill whose SKILL.md has no metadata.keeper_proposal key is a ",
            "person's — they wrote it or adopted it. A patch or archive you propose for one is never ",
            "applied by itself: it waits for that person's review. A skill you create is offered to no ",
            "session until a person adopts it.\n\n",
        )
    };
}

/// `_MEMORY_REVIEW_PROMPT`, adapted.
pub const MEMORY_REVIEW_PROMPT: &str = concat!(
    "Review the conversation above and consider saving to memory if appropriate.\n\n",
    "Memory has ",
    memory_routing_block!(),
    "If something stands out, propose it once, in the right store, using memory_propose with the ",
    "matching target. If nothing is worth saving, just say 'Nothing to save.' and stop."
);

/// `_SKILL_REVIEW_PROMPT`, adapted.
pub const SKILL_REVIEW_PROMPT: &str = concat!(
    "Review the conversation above and update the skill library. Be ACTIVE — most sessions produce ",
    "at least one skill update, even if small. A pass that does nothing is a missed learning ",
    "opportunity, not a neutral outcome.\n\n",
    "Target shape of the library: CLASS-LEVEL skills, each with a SKILL.md of always-on rules. Not a ",
    "flat list of narrow one-session skills. This shapes HOW you update, not WHETHER you update.\n\n",
    lesson_layer_block!(),
    "Signals to look for (any one of these warrants action):\n",
    "  • User corrected your style, tone, format, legibility, or verbosity. Frustration signals ",
    "like 'stop doing X', 'this is too verbose', 'don't format like this', 'why are you ",
    "explaining', 'just give me the answer', 'you always do Y and I hate it', or an explicit ",
    "'remember this' are FIRST-CLASS skill signals, not just memory signals. Update the relevant ",
    "skill(s) to embed the preference so the next session starts already knowing.\n",
    "  • User corrected your workflow, approach, or sequence of steps. Encode the correction as a ",
    "pitfall or explicit step in the skill that governs that class of task.\n",
    "  • Non-trivial technique, fix, workaround, debugging path, or tool-usage pattern emerged ",
    "that a future session would benefit from. Capture it.\n",
    "  • A skill that got loaded or consulted this session turned out to be wrong, missing a step, ",
    "or outdated. Patch it NOW.\n\n",
    "Preference order — prefer the earliest action that fits, but do pick one when a signal above ",
    "fired:\n",
    "  1. UPDATE A CURRENTLY-LOADED SKILL. Look back through the conversation for skills you read ",
    "via skill_view. If any of them covers the territory of the new learning, PATCH that one first: ",
    "skill_propose with op 'patch' and the whole new SKILL.md as body (re-load it with skill_view ",
    "during this review — see Read-before-write below). It is the skill that was in play, so it's ",
    "the right one to extend.\n",
    "  2. UPDATE AN EXISTING UMBRELLA (via skills_list + skill_view). If no loaded skill fits but ",
    "an existing class-level skill does, patch it. Add a subsection, a pitfall, or broaden a ",
    "trigger.\n",
    "  3. CREATE A NEW CLASS-LEVEL UMBRELLA SKILL when no existing skill covers the class: ",
    "skill_propose with op 'create' and the whole SKILL.md as body. The ",
    "name MUST be at the class level. The name MUST NOT be a specific PR number, error string, ",
    "feature codename, library-alone name, or 'fix-X / debug-Y / audit-Z-today' session artifact. ",
    "If the proposed name only makes sense for today's task, it's wrong — fall back to (1) or (2).\n\n",
    keeper_skill_rules!(),
    "User-preference embedding (important): when the user expressed a style/format/workflow ",
    "preference, the update belongs in the SKILL.md body, not just in memory. Memory captures 'who ",
    "the user is and what the current situation and state of your operations are'; skills capture ",
    "'how to do this class of task for this user'. When they complain about how you handled a ",
    "task, the skill that governs that task needs to carry the lesson.\n\n",
    "If you notice two existing skills that overlap, note it in your reply — the background ",
    "curator handles consolidation at scale.\n\n",
    "Do NOT capture",
    do_not_capture_block!(),
    "'Nothing to save.' is a real option but should NOT be the default. If the session ran ",
    "smoothly with no corrections and produced no new technique, just say 'Nothing to save.' and ",
    "stop. Otherwise, act."
);

/// `_COMBINED_REVIEW_PROMPT`, adapted.
pub const COMBINED_REVIEW_PROMPT: &str = concat!(
    "Review the conversation above and update two things:\n\n",
    "**Memory**: ",
    memory_routing_block!(),
    "**Skills**: how to do this class of task. Be ACTIVE — most sessions produce at least one ",
    "skill update. A pass that does nothing is a missed learning opportunity, not a neutral ",
    "outcome.\n\n",
    "Target shape of the skill library: CLASS-LEVEL skills with a SKILL.md of always-on rules — not ",
    "narrow one-session skills.\n\n",
    lesson_layer_block!(),
    "Signals that warrant a skill update (any one is enough):\n",
    "  • User corrected your style, tone, format, legibility, verbosity, or approach. Frustration ",
    "is a FIRST-CLASS skill signal, not just a memory signal. 'stop doing X', 'don't format like ",
    "this', 'I hate when you Y' — embed the lesson in the skill that governs that task so the next ",
    "session starts fixed.\n",
    "  • Non-trivial technique, fix, workaround, or debugging path emerged.\n",
    "  • A skill that was loaded or consulted turned out wrong, missing, or outdated — patch it ",
    "now.\n\n",
    "Preference order for skills — pick the earliest that fits:\n",
    "  1. UPDATE A CURRENTLY-LOADED SKILL. Check what skills were read via skill_view in the ",
    "conversation. If one of them covers the learning, PATCH it first with skill_propose (re-load ",
    "it with skill_view during this review — see Read-before-write below). It was in play; it's ",
    "the right place.\n",
    "  2. UPDATE AN EXISTING UMBRELLA (skills_list + skill_view to find the right one). Patch it.\n",
    "  3. CREATE A NEW CLASS-LEVEL UMBRELLA when nothing exists. Name at the class level — NOT a ",
    "PR number, error string, codename, library-alone name, or 'fix-X / debug-Y' session artifact. ",
    "If the name only fits today's task, fall back to (1) or (2).\n\n",
    keeper_skill_rules!(),
    "User-preference embedding: when the user complains about how you handled a task, update the ",
    "skill that governs that task rather than memory. Memory says 'who the user is and ",
    "what the current situation and state of your operations are'; skills say 'how to do this ",
    "class of task for this user'. A user-preference lesson lives in exactly ONE place: the skill ",
    "that governs the task when one exists, USER.md only for cross-cutting preferences no skill ",
    "owns — never both. Duplicating it is how a memory file ends up restating SKILL.md until both ",
    "hit their size limits.\n\n",
    "If you notice overlapping existing skills, mention it — the background curator handles ",
    "consolidation.\n\n",
    "Do NOT capture as skills",
    do_not_capture_block!(),
    "Act on whichever of the two dimensions has real signal. If genuinely nothing stands out on ",
    "either, say 'Nothing to save.' and stop — but don't reach for that conclusion as a default."
);

/// Which nudges fired: the prompt a review pass is handed.
pub fn review_prompt(memory: bool, skills: bool) -> Option<&'static str> {
    match (memory, skills) {
        (true, true) => Some(COMBINED_REVIEW_PROMPT),
        (true, false) => Some(MEMORY_REVIEW_PROMPT),
        (false, true) => Some(SKILL_REVIEW_PROMPT),
        (false, false) => None,
    }
}
