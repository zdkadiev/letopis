---
name: letopis-capture
description: Use when a user invokes /capture or asks to retain a finding, decision, trade-off, or observation from prior work in the project's Letopis knowledge base.
---

# Capture Letopis knowledge

Capture what will help someone make a decision, understand the design, avoid a pitfall, or verify a claim in future work. Use the conversation and artifacts from the relevant prior work. Read the [Letopis authoring skill](../letopis/SKILL.md) for shared markup, links, source snapshots, and validation.

## Choose the mode

| Request | Action |
| --- | --- |
| `/capture` or a general request to capture useful knowledge | Propose entries for review. Wait for the user to confirm which entries to save. |
| `/capture yolo` | Select the durable findings and save them directly. |
| `/capture <what to capture>` | Save the requested knowledge directly, limited to the user's subject. Honor an explicit request to review it first. |

In review mode, show each proposed entry's claim, rationale or limits, evidence, and intended file or section. Keep the proposal in the conversation. After approval, apply only the approved entries and incorporate the user's corrections. If new evidence materially changes an approved claim, bring that change back for review.

## Prepare and save

1. Verify the claim against available code, accepted specs, tests, measurements, issues, PRs, or explicit decisions. State the scope and conditions that make it true. Preserve the status of each claim as an accepted decision, observed result, or open proposal. Include the rationale and rejected alternatives when they explain a lasting trade-off. If the conversation is the only evidence for a user decision, attribute it explicitly and link to it if a stable reference is available. Omit unsupported conclusions and incidental task history.
2. Choose the destination using [the placement rules below](#place-the-finding). Identify it before presenting a proposal or saving an entry.
3. When the selected mode authorizes saving, write marked Markdown. Add `external` links to available evidence and use a verified `source_snapshot` for local source bindings as described in the authoring skill. Link to existing elements with a meaningful `related_to` or `includes` edge where applicable, using actual element IDs. Preserve existing IDs and `created_at` values on updates.
4. Run `letopis check` and review `result.errors`. [Inspect the affected IDs](../letopis-search/SKILL.md#read-snapshot-evidence) and read their captured passages to confirm the saved content. If the CLI cannot verify the index, check local markup and link targets directly. Report any unresolved findings or verification gaps affecting the saved entries.

## Place the finding

- Find the owning subject in the project's documented knowledge directory, defaulting to `letopis/`. Use [Letopis search](../letopis-search/SKILL.md) and its Markdown fallback. Check neighboring concepts under their domain, implementation, integration, and troubleshooting terms. A finding from a test or decision can belong in a concept filed under a different name.
- Skip an accurate duplicate. Extend the owning element when the finding answers the same question. Add a marked section to a section-based file when the subtopic needs its own explanation or source basis. Keep the claim with its rationale, limits, examples, pitfalls, and supporting sources.
- Create a file when the subject stands on its own. Reuse a suitable folder and keep the hierarchy shallow. Name the file for the subject using the project's naming convention, or use a lowercase hyphenated Markdown filename such as `cache-key.md`.

## Report the result

Keep the report short. After saving in any mode, list what was added or updated and link to the exact file or section. Mention any unresolved verification gap. When awaiting approval, link or name the proposed destinations and say that nothing has been added yet. If nothing qualifies or it is already recorded, say so and link any matching existing entry.
