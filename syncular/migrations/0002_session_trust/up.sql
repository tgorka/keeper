-- session trust (D-34): `untrusted` once the session has read outside content — a web search, a
-- fetched page, an MCP server's answer. Absent is `trusted`. It only rises: nothing lowers it.

ALTER TABLE sessions ADD COLUMN trust TEXT;
