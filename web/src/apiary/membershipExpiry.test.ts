import { expect, test } from "vitest";
import type { ApiaryMember } from "../api";
import { membershipNote } from "./membershipExpiry";

const DAY = 86_400;
const NOW = Date.UTC(2026, 9, 14, 12) / 1000;
const member = (credential_expires_at: number | null, is_local = false): ApiaryMember => ({
  hive_id: "h", hive_name: "Angie's Hive", operator_id: "o", operator_display_name: "Angie", role: "member", is_local, credential_expires_at,
});

test("a member in good standing says nothing about its membership", () => {
  expect(membershipNote(member(NOW + 20 * DAY), NOW)).toBeUndefined();
  expect(membershipNote(member(null), NOW)).toBeUndefined();
  expect(membershipNote(member(NOW - DAY, true), NOW)).toBeUndefined();
});

/** ⚠️ ADR 0117: the expiry is the cause, so the roster names it and what brings it back. */
test("an expired member says when, and that it reconnects once it updates", () => {
  const note = membershipNote(member(NOW - 3 * DAY), NOW);
  expect(note?.expired).toBe(true);
  expect(note?.text).toMatch(/^Membership expired .+\. It reconnects by itself once it updates, until .+\.$/);
  expect(membershipNote(member(NOW - 61 * DAY), NOW)?.text).toMatch(/too long ago to renew\. Remove it and invite it again\.$/);
});

test("a member inside its last fortnight that has not renewed is warned about", () => {
  const note = membershipNote(member(NOW + 8 * DAY), NOW);
  expect(note).toEqual({ expired: false, text: expect.stringMatching(/^Membership expires .+\. It renews by itself once it updates\.$/) });
});
