# 0147 — A task listing comes a page at a time

- Status: Accepted
- Date: 2026-11-02
- Decided by: the operator (2026-11-02). Asked how a listing should keep
  fitting as the board grows, they chose "Pages": each listing stops at about
  40,000 characters and names the next page, and the closed list drops from
  50 to 20.
- Amends: ADR 0143. The closed index names 20, on page 0 only.
- Amends: ADR 0145. A worker's own list is paged too, and its estimate of
  four bytes a token was wrong for listings.
- Replaces: the 2 MB bound on a listing's open tasks and the `omitted` list
  of what did not fit.

## Context

On 2026-11-02, at 61 open tasks, Claude Code refused a whole-board listing
of 56,215 characters with "exceeds maximum allowed tokens". The reader got no
board at all. The listing was already brief (ADR 0143):

| Part | Characters |
| --- | --- |
| 60 open tasks in brief | 41,031, about 680 each |
| the reader's own task in full | 2,844 |
| closed index, 50 entries | 11,507 |
| the rest of the reply | 815 |

What Claude Code 2.1.316 does with a tool reply, measured with a throwaway
MCP server answering the way `swarm_list_tasks` does:

| Reply text | What reached the model |
| --- | --- |
| 29,800 to 49,931 characters | the whole reply |
| 51,671 and 54,924 of the listing; 61,174 of prose | a 2 KB preview and a file to read ("Output too large") |
| 56,215 of the listing; 81,530 and 101,902 of prose | nothing: refused for its tokens |

So the first limit is about 50,000 characters whatever the content. The token
refusal comes later, and sooner for JSON than for prose. For this listing it
falls between 54,924 and 56,215 characters, about 2.2 characters a token at
the documented 25,000-token default, not the four ADR 0145 estimated.

A trim would only move the wall. Dropping the least useful brief fields
saves 18% an entry, and the board passes the limit again a few dozen tasks
later.

## Decision

1. **A task listing comes a page at a time.** Each page's whole reply text
   is at most 40,000 bytes. Bytes are never fewer than the characters Claude
   Code counts, so this leaves a fifth of headroom under the 50,000 where
   previews start.
2. **Pages are cut in board order.** A page ends where the next entry would
   not fit, so each open task is on exactly one page. The page's own
   fields are measured at their widest, every note present and every number
   as wide as the board allows, so a page never comes back larger than the
   one measured.
3. **The reply says where it sits.** `page`, `pages` and `open_total` always;
   `next_page` while there is one; and `paging_note` when there is more than
   one page, saying to read every page. `swarm_list_tasks` takes `page`, from
   0. A page past the last lists nothing and still counts the pages.
4. **Page 0 names the 20 most recently closed tasks**, within 8 KB, with the
   total. Later pages carry no closed index.
5. **A worker's own list, and the list of tasks a tool filed, are paged the
   same way.**
6. **An entry too large for a page on its own is named in its place** by id,
   title and state with `oversized: true`, and `oversized_note` sends the
   reader to `swarm_read_task_history`. A description may be 100,000 bytes, so
   work in hand listed in full can be larger than a page.
7. **An assignment tells the worker to stop only if its task is on none of
   the listing's pages.** It said "if this task is not visible", and a worker
   reading only page 0 would have stopped work listed on page 1.

## Consequences

- The board that was refused is two pages: two calls where there was one
  refused one.
- A page is cut when it is read, so a task can move between pages if the
  board changes between two reads. The note says to compare `open_total` and
  `observed_at`. A cursor would hold steadier and is left out until a reader
  is seen missing a task this way.
- Agents connected before this hold an older tool list, so the tool surface
  revision moves.

## Not proven

- **The Keeper serving pages.** The refused listing's own entries, run
  through the new paging, made pages of 39,316 and 11,396 characters, and
  Claude Code 2.1.316 passed both to the model whole. The live board is
  measured after the reload.
- **Whether agents read past page 0.** The tool description and the note say
  to; nothing here shows Queen or a worker does.
- **Other answers.** This bounds task listings. A task read in full with
  `swarm_read_task_history` can still be larger than an agent reads.

## Evidence

Tests:

- `every_page_of_a_large_board_fits_and_the_pages_list_it_once`: 240 open
  tasks come back in at least three pages, each at most 40,000 bytes of text
  and every page but the last over 90% full; together they list every open
  task once in board order; only page 0 names closed work.
- `a_workers_own_list_is_paged_and_keeps_its_work_in_full`: 60 Ready tasks
  over at least two pages, each within the bound, each task once and in full.
- `a_task_too_large_for_a_page_is_named_and_the_page_still_fits`: a 75 KB
  description is named with `oversized` and the note, and the page fits.
- `a_connections_filed_list_is_paged_like_the_board`: 40 filed tasks in full
  over at least two pages, each within the bound, each once.
- `a_page_past_the_last_lists_nothing_and_counts_the_pages`.
- `a_page_ends_where_the_next_entry_would_not_fit`: order kept, page 0's
  smaller budget honoured, an oversized entry named in place.
- `queen_reads_the_board_in_brief_with_the_last_twenty_closed` and
  `a_board_listing_names_closed_work_without_carrying_it`.
- `a_brief_without_a_ruling_is_unchanged`: the assignment says "on none of
  the listing's pages".

Ablations, each failing its test, with the file restored byte for byte: the
old 2 MB bound; the closed index left out of page 0's measure; the closed
index on every page; oversized entries not named; never starting a new page;
the `page` argument ignored; pages cut at half their budget; 50 closed; no
`next_page`; a worker's list unpaged; a tool's filed list stuck on page 0;
the old assignment text.
