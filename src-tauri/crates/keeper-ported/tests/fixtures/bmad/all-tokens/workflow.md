# All tokens — {{config.core.project_name}}

Speak {{.communication_language}} to {{.user_name}}; write {{.document_output_language}}.

- Plans: {{config.modules.bmm.planning_artifacts}}
- Builds: {{.implementation_artifacts}}
- Knowledge: {{config.modules.bmm.project_knowledge}}
- Test design: {{config.modules.tea.test_design_output}}

Before you start:

{workflow.activation_steps_prepend}

Facts:

{workflow.persistent_facts}

Nothing extra: {workflow.activation_steps_append}

Handoff: {workflow.implementation_handoff}

Open spec: `{workflow.open_spec}` — on complete: `{workflow.on_complete}`.

## Review

{workflow.review_layers}

## One-shot review

{workflow.oneshot_review_layers}

Next: read [[bmad-snapshot:steps/step-01-gather.md]], then [[bmad-snapshot:steps-notes.md]].
Not a token: {{config.}} {{.}} {workflow.} [[bmad-snapshot:steps/]] {{{.user_name}}} {skill-root}.
