# Dream audit and repair workflow

## Build the baseline

Run from the Git root. Record the current worktree state and keep the baseline output outside authored knowledge. Use the local Letopis executable and its command help. A read or check can refresh generated state. Review the authored files directly when the index is absent or a command fails. Retain the failed or skipped check in the report.

```sh
git status --short
printf '%s\n' '{}' | letopis check
python3 "$dream_skill_dir/scripts/check_navigation.py" letopis
```

Set `dream_skill_dir` to this skill's directory. Pass the actual knowledge folders or files to the navigation checker. With no arguments it checks `letopis/`. Run the [size audit](../../letopis/scripts/audit_knowledge.py) from the Git root. Its JSON reports direct concept-file counts by folder and flags folders over 40 concept files, elements over 40 lines, files over 800 lines, and mixed layouts. Treat these as review leads. Use the [check wrapper](../../letopis/scripts/check_links.py) to group Letopis diagnostics by reason. The wrapper exits 1 for findings and 2 for a failed check.

For graph and local source validation, require exit 0, `ok:true`, and an explicit `result.problems` array. An empty array means there were no findings in the selected scope. A nonempty array shows up to 100 findings. For `source_changed`, review every address in `details.targets`. An empty or omitted `paths` request also audits complete source snapshot trees. Scoped findings, `status`, and ordinary reads have narrower guarantees. Investigate parse errors in live Markdown first because invalid files can retain their last valid indexed projection. Check discovery settings and ignores if expected documents are absent.

The navigation checker uses Pandoc's GFM parser with heading attributes. It checks relative local paths with exact filename spelling, Markdown heading and HTML anchors, and inclusive `#Lx` or `#Lx-Ly` ranges. It flags marked knowledge links whose destination fragment disagrees with the canonical anchor. It includes inline and reference links, local Markdown images, and HTML links in Markdown. It scans regular Git-tracked and nonignored untracked Markdown as well as explicitly named Markdown files. Include files admitted by custom discovery settings explicitly when needed. It leaves URL availability, viewer-specific fragment rules, and anchors in other file formats for separate review. Exit 0 means no findings, 1 means local navigation findings, and 2 means the helper could not complete. If Pandoc is unavailable, inspect local navigation and backlinks directly and describe the coverage you verified.

## Review the knowledge

1. Review what each folder promises, what each file owns, and what question each marked section answers. Include unmarked navigation when it shapes the topic. For a large corpus, review one group at a time and keep a shared inventory of concepts and overlapping claims. A folder is a parent concept and can contain both concept files and child concept folders. An indexed overview can represent that grouping with acyclic `includes` edges to file elements and child overviews.
2. Compare nearby sections, search for synonyms and similarly scoped claims, and follow both relation directions. Read authoritative code, contracts, decisions, or reproducible observations. Compare version, environment, lifecycle stage, and scope before resolving an apparent contradiction. A source hash establishes which bytes were read. Check what those bytes actually support.
3. Correct a claim against current evidence, qualify it if both versions are valid in different contexts, or record the conflict if evidence is inconclusive. Treat age as a prompt for review and use the [concept boundaries](#split-or-merge-concepts) below to plan reorganizations.
4. Retire superseded or obsolete material when the current evidence shows the old behavior is gone and the record has no durable value. Keep the parts that still explain a live constraint, decision, or compatibility requirement. Cite the reason for retirement and choose a replacement for each incoming link when one exists. Remove an obsolete pointer when its referring sentence remains accurate without it. Check for unique material before deleting a file or emptying a folder.

Use [Search's source filters](../../letopis-search/SKILL.md#search-the-index) with `needs_recheck:true` on relevant queries and examine `traverse` flags for `source_changed`. Search results are bounded. Compare the written claim with the addressed evidence in the recorded tree. Report source drift separately from broken provenance, including when a current claim still agrees with its original evidence.

### Split or merge concepts

- Define each boundary by the subject and the question it answers. Keep independently useful concepts separate even when they share packages or sources. Keep each explanation with its qualifications and evidence.
- Aim for roughly 30–40 readable lines per element when the subject warrants it. Split a broad element into marked sections in the same file when its subtopics share an owner. Use separate files when the subtopics have distinct purposes or serve distinct readers.
- Aim for roughly 200–800 lines per concept file when there is enough related material. Consider combining sibling files under 200 lines into a shared concept file with separate marked elements. Above 1,000 lines, look for substantive subtopics that could own separate files. These sizes guide review. Short, cohesive concepts can stand on their own.
- Count the marked Markdown files directly in each folder. Each subfolder gets its own count. A folder with 30–40 concept files is reasonable. Above 40, review whether its subject has become broad enough to split into named subfolders. Keep files that explain the parent concept alongside the child folders when that helps readers.
- Merge small sibling folders when their contents fit one concept. Use the resulting direct file count and subject cohesion to evaluate the merge. Map moved IDs and update backlinks when splitting or merging folders.

## Plan changes and their links

Record a short row per repair with the problem, evidence, proposed change, affected files and IDs, and how to verify it. For moved or retired material, record the old ID and Markdown destination with their replacements or retirement actions. Preserve stable published IDs wherever the existing concept still makes sense. A useful parent can keep its ID and link to new child concepts. A folder move changes all contained file and element paths.

Before editing, inspect the incoming graph of affected IDs. For example:

```sh
printf '%s\n' '{"element_ids":["letopis/queue.md#retry"],"mode":"both","direction":"incoming","depth":1,"limit":1000,"edge_limit":3000}' | letopis traverse
python3 "$dream_skill_dir/scripts/check_navigation.py" --target letopis/queue.md
```

Use `--target` for each file that will move, be deleted, or change anchors. This scans Git-visible Markdown across the repository by default and returns backlinks even to a deleted file. Check the relevant old fragments when a file keeps other sections. Search current markup across the repository for canonical paths, old IDs, and external source addresses into the affected files. Traverse is bounded and can skip broken edges. Include off-scope referrers necessary to complete a planned repair. Preserve references that deliberately point to a historical Git revision.

## Apply and recheck

- Re-read affected files and their evidence before applying a previously written plan. Reconcile changes to the worktree or the evidence. Keep unrelated edits.
- Keep each moved element's `created_at`, explicit anchor where applicable, and supporting sources. When combining two elements, use the surviving element's metadata and keep separate sections if their evidence needs different source trees. Rebase relative Markdown links in moved content, even when their canonical addresses stay the same. Update both the ordinary destination and the canonical title for affected incoming edges. Review `includes` cycles, self-links, and duplicate edges introduced by merges.
- Keep `source_snapshot` when the written claim and its recorded evidence agree, including after editorial changes. Replace the basis only when a claim or binding requires different evidence. Verify every local binding against the replacement tree using the [source basis procedure](../../letopis/SKILL.md#record-an-honest-source-basis). Report unreadable Git objects or uncertain evidence as open findings.
- Verify each coherent batch and finish with unscoped `check` and the navigation audit. Run the backlink scan again for old and new paths. Confirm that obsolete destinations and edges are gone, remaining referrers resolve, and intended `traverse` triples exist in their correct direction. Use a query or explicit Markdown reading to confirm expected concepts and updated claims. Compare findings with the baseline and review Git's diff, including new untracked files.

For a changed group, inspect the surviving knowledge IDs and the local source addresses that support them in one request. For an audit with no edits, sample representative knowledge and its local evidence from the reviewed scope. For example:

```sh
printf '%s\n' '{"element_ids":["letopis/queue.md#retry","src/queue.rs#L2-L4"]}' \
  | letopis inspect \
  | python3 "$dream_skill_dir/scripts/verify_snapshot.py"
```

The verifier checks the SHA-256 of exact addressed Git tree bytes, preserving line endings. Exit 1 identifies an unavailable or mismatched local address. Exit 2 indicates the inspect response, tree, or command could not be used. Remote source addresses are listed separately as unverified. Check that all requested local IDs were verified and read the passages from the reported tree. Use the unscoped `check` for closure verification of effective historical bases and `inspect` for the current reading snapshot.

For a clean outcome, confirm that the requested scope was reviewed, the checks covered it, and all material findings have been resolved. Include any out-of-scope findings from the global check in a scoped report. For other results, report what changed, where it changed, and which checks or evidence remain open.
