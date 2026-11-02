# 0144 — Agent tools count what they cost

- Status: Accepted
- Date: 2026-11-02
- Decided by: the operator, on the optimization pass before the next release
  (2026-11-02). They chose "Do everything listed first", and its fourth item
  is "a per-tool cost sweep over the MCP tools, to find the next hot spot".

## Context

The agent surface has about 60 tools. Nothing recorded which of them agents
call, how often, how long each answer takes, or how large it is. The API logs
no per-request lines, and no table holds tool calls.

So the earlier costly tools were found by accident. A whole-board listing was
noticed at 291 KB because it overflowed an agent's tool output (ADR 0143),
and a Queen summary was found restating whole tasks into a poll she runs
thousands of times a day. A sweep built the same way would find only what
already hurts.

## Decision

1. **Each answered tool call is counted against its tool name**: calls,
   failures, time to answer, and the size of the serialized answer, with the
   largest and longest single call. A failure is a refused call or an answer
   marked as an error.
2. **The tally is bounded by name.** The caller chooses the name, and an
   unknown name is refused only after it is counted. So at most 160 names are
   kept, more than twice the surface, each at most 64 bytes, about twice the
   longest real name. Calls to any further or longer name are counted
   together in one row.
3. **It keeps counts only.** No arguments, answers or caller identities.
4. **It lives in the API process and is lost on restart.** The application
   state owns it. A reading says when counting started.
5. **`GET /api/v1/runtime/agent-tools` reads it, most time first.** It is
   authorized like every other runtime reading.

## Consequences

- The sweep reads what agents actually call over hours of real work, rather
  than sizing each tool in isolation.
- Measuring an answer's size serializes it once more. That is the same work
  the server already does to send it, and the time recorded excludes it.
- A count from before a restart is gone. The sweep reads over a known window
  after a reload.

## Not proven

- Which tool is the next hot spot. That is what the tally exists to find, and
  it needs a reload and some hours of use.
- The operator tool surface (`ops_mcp`) answers through its own handler and
  is not counted. Nothing here measures it.

## Evidence

Tests:

- `each_tool_call_is_counted_with_the_size_of_its_answer`: a listing through
  the real MCP handler is counted once, its size matches the answer the agent
  reads, and an unknown tool is counted as one failure.
- `names_past_the_capacity_are_counted_together`: 185 made-up names leave 160
  rows and one overflow row of 25 calls, and a name seen before the bound
  keeps its own row.
- `a_name_longer_than_any_tool_is_counted_with_the_others`: a 65-byte name
  goes to the overflow row and a 64-byte name keeps its own.
- `a_tool_is_counted_by_calls_time_and_bytes_with_the_costliest_first`.
- `the_agent_tool_tally_is_an_authenticated_runtime_reading`: refused without
  the operator's token, read with it.
