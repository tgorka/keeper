-- the tools an instance offers (R-HQ-10, R-HQ-30): a JSON array of tool names, its definition's
-- tools less those its container withholds, written by the instance when it starts. A session's
-- `session_tool_switches` can only switch these off. Absent: the instance has not said, and a
-- client offers no switch.

ALTER TABLE agent_profiles ADD COLUMN tools JSON;
