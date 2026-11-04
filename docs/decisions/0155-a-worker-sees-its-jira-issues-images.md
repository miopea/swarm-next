# 0155 — A worker sees its Jira issue's images

- Status: Accepted
- Date: 2026-11-04
- Decided by: the operator's answer to a worker that could not read a report:
  "The Jira connection should allow you to pull everything down via that
  connector". The tool is scoped like the Jira comments a worker already
  reads, so no new authority was put to the operator.
- Builds on: the operator's Jira task detail, which already fetches an issue's
  images for the task card.

## Context

A reporter put the error only in two screenshots on a Jira issue. The worker
holding the task read the issue through Swarm and got the text "[attachment]"
twice, with no id, name or content:

- Jira's rich text marks an image as a media node. `jira_document_text` turns
  it into "[attachment]", or "[attachment: name]" when the node has one.
- A worker's only Jira tools were `swarm_list_jira_comments` and
  `swarm_comment_jira_task`.
- The bytes were reachable only through the operator's HTTP route behind the
  operator token, which the task card uses.

So the worker had to ask the operator what the screenshots said, about a
report the operator had routed to it so it could read it.

## Decision

1. **A new worker tool, `swarm_read_jira_attachment`.** It takes a task id and
   an optional attachment id.
   - Without the attachment id, or with an empty one, it lists the files on the
     task's linked Jira issue: id, filename, media type, size and whether it is
     an image. That includes images pasted into comments, which Jira keeps on
     the issue.
   - With an attachment id it returns that image as an MCP image block, so the
     model sees it. A short text line names the issue, file and size.
2. **The same scope as the comments a worker reads.** The task goes through
   `visible_task_id`: a worker reaches only its own assignment, and Queen any
   Hive task. The id is then looked up in that issue's own attachment list,
   so one task cannot reach another issue's files by naming their ids. The
   download goes through the adapter's existing `attachment()`, which makes the
   same membership check again.
3. **The adapter's existing limits, and one more.**
   - An issue with more than 40 attachments is not listed at all: the adapter
     refuses it with its bounded-limit error, as it does for the task card.
   - Only PNG, JPEG, GIF and WebP can be shown, which is both what the adapter
     downloads and what the model accepts.
   - An image is shown only up to 3,932,160 bytes, whose base64 is exactly
     5 MiB. Claude Code states 5 MiB of base64 as the API's per-image limit
     (`maxBase64Size: 5242880`, read from its 2.1.318 binary).
   - Jira's declared size is checked before the download and the real size
     after it, because the declared size is only a claim.
4. **A refusal says why.** A missing id names the issue and says to list. A
   file that is not an image names its type. One that is too large names its
   size, the limit, and that the operator can open it on the task card.
5. **A read starts no delivery pass.** Like the other reads, a successful call
   does not wake the control room or run a coordination pass (ADR 0138).

`swarm_list_jira_comments` now says that "[attachment]" is read with the new
tool. The tool surface revision moves to 38.

### Why the size limit holds even though Claude Code can shrink images

The same Claude Code binary also passes an image a tool returns through its
own resizer. So for a Claude worker the limit refuses some images, roughly 4 to
6 MB, that the client could have shrunk. It is kept anyway:

- Swarm serves other providers' workers, and older Claude Code releases, and
  nothing here knows which one is asking.
- An image the API refuses stays in the conversation, so it could fail every
  request after it. Refusing it here costs one tool call.

The limit is on bytes only. It does not check pixel dimensions, so it does not
guarantee that the API accepts every image under it.

## What this does not change

- The operator's task-card route and its limits are unchanged.
- Jira text still says "[attachment]". The media node's id belongs to Atlassian
  Media, not to the issue's attachment list, so the two cannot be matched. The
  listing gives the files instead.
- Swarm writes nothing to disk. The client that receives the image may keep it
  in its own transcript.

## Consequences

- A worker can read a screenshot on its own task without asking the operator.
- Each image read costs two issue reads and one download.
- The tool-cost record (ADR 0144) counts only an answer's text, so an image
  answer is counted at the size of its one text line.
- A running worker sees the tool once its session refreshes its tool list.

## Validation

- `a_worker_sees_its_own_jira_issues_images_and_no_others` (agent), against a
  fake Jira:
  - the listing returns all four files on the issue with images marked;
  - an image comes back as an image block with the exact bytes, also when its
    id has spaces around it, and an empty id lists;
  - a text file is refused by type, and an image declared over the limit is
    refused by size without being downloaded;
  - an image declared small that arrives over the limit is refused by size;
  - another issue's attachment id through the worker's own task is refused;
  - another task, listed or read, is refused;
  - only the allowed downloads reached Jira;
  - none of the calls started a delivery pass.
- The worker tool list test now includes the tool, and the tool surface
  fingerprint moved.
- Not tested: an issue with more than 40 attachments, a Jira failure, a task
  with no Jira link, and Queen's access.
- Not yet observed on a live Jira issue through a running worker.
