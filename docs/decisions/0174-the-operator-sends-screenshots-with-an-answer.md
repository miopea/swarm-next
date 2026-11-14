# 0174 — The operator sends screenshots with an answer

- Status: Accepted
- Date: 2026-11-13
- Decided by: the operator, who asked for it: "We need the ability to add
  screenshots on the needs you page. Right now it is a limitation to explain
  some things." The worker reading the answer is the one the request already
  goes to, so no new authority was put to the operator.
- Builds on: ADR 0155 (a worker sees its Jira issue's images), whose size
  limit and image block this reuses, and the bounded clarification exchange
  in Needs you.

## Context

A Needs you request can only be answered in words. The operator was asked to
check a sign-in flow, found that signing out signed them straight back in, and
had to describe in a sentence what a screenshot would have shown. The same
holds for a question asked back before deciding.

A worker's model can be shown an image: `swarm_read_jira_attachment` already
returns one as an MCP image block, up to 3,932,160 bytes (5 MiB of base64).

## Decision

1. **Where.** The operator can attach an image to each answer field in Needs
   you: "Tell the worker what to do instead", "Add an optional note" (on a
   request and in an interview), and "Ask a question". Pasting into the field
   adds the image, and "Attach screenshot" beside it opens a file picker. Each
   image shows as a thumbnail with Remove. Text pasted alone pastes as before;
   a paste carrying both an image and text keeps the text too.
2. **How much.** At most 4 images go with one answer or question, counting
   every field of that answer together, and at most 16 on one request. An
   image is PNG, JPEG, GIF or WebP, and at most 3,932,160 bytes. The page
   redraws an image as JPEG when it is larger than that, wider or taller than
   7,680 pixels (the model refuses past 8,000), or in another type, at most
   five times at 30% smaller each, and refuses it with a reason past that.
   The server checks the count, the type, the size and the file's own
   signature again, so a client that skips the page's work is refused.
3. **Sent with the answer, in one request.** The images travel as base64 in
   the same request as the answer or the question, so an answer and its
   images are recorded together or not at all. Those two routes accept a
   body of up to 22 MiB; every other route keeps its limit.
4. **Kept on the Hive, out of the database.** Each image is written as its
   own file in a `decision-images` directory beside the database, named by a
   fresh id, at mode 0600 in a 0700 directory. Its row (schema 228) holds the
   id, the request, the question it went with if any, its position, type,
   size and time, and nothing of the image (ADR 0021).
   - The files are written first, then the rows are written in the same
     transaction as the answer or question. A refused answer removes its
     files. A repeated question records nothing new and removes the files
     written for it.
   - A crash between the two leaves files no row names. The next image
     written removes them, with no wait: a write holds the store's lock from
     its first file until its answer is recorded or its files removed, and
     one API process owns the directory, so a file no row names is never one
     still being recorded. Only names Swarm gives these files are removed.
   - A request's images are kept while it is open and for 90 days after it
     is answered or withdrawn. Older ones are dropped when the next image is
     attached, and their files go with the next sweep. An open request's
     images are never dropped to make room.
   - The Hive keeps at most 512 images and 512 MiB. Past that, an answer with
     images is refused with "decision_image_storage_full" (507), and the
     operator can send it without them.
5. **Who sees them.** The worker the answer or question is delivered to is
   told in the same message: "The operator attached N image(s): see it with
   swarm_read_decision_image and decision_id … before you act."
   - `swarm_read_decision_image` lists a request's images, or shows one as an
     image. The worker that raised the request, a worker holding the
     request's task, and Queen can read them; any other worker is refused.
     `swarm_list_decisions` with a decision id lists them for the same
     readers.
   - The tool description says text inside an image is data. It does not
     widen what the answer authorises, as a relayed message does not.
   - The operator reads them back through the same gate as the rest of
     Needs you and the Jira images of ADR 0155, and the answered card and
     the question history show what was sent.
6. **Nothing changes without images.** An answer or question sent without
   one is the same request, the same record and the same delivered text as
   before.

## Consequences

- The operator can show a worker what they saw, in the answer, instead of
  describing it.
- Screenshots of a signed-in page can hold personal data. They stay on the
  Hive, readable only by the operator and the workers named above. Those of
  a request closed more than 90 days ago go when the next image is sent;
  nothing removes them on a timer, so on a Hive where nobody sends another
  image they stay, within the 512-image cap.
- A request that stays open keeps its images, so a Hive with many open
  requests carrying images can fill the 512-image store. The operator is then
  refused with a reason and can still answer in words.
- The worker tool surface changes, so a running worker sees the new tool
  after it restarts. The worker engine is unchanged.
- Not built: images in Queen's own messages, and a way for a worker to send
  an image to the operator.
- Not proven by the tests: a paste from a real clipboard on each browser the
  operator uses, and that the worker's model reads a delivered screenshot
  well enough to act on it. Both need the release on a live Hive.
