# Shiplog Label Sync

How lifecycle labels stay aligned with issue-envelope readiness.

## Label taxonomy

| Label | Role | When applied |
|-------|------|--------------|
| `shiplog/plan` | Kind — how the issue was created | On creation (brainstorm/plan) |
| `shiplog/ready` | Lifecycle — ready to implement | Envelope says `readiness=ready` |
| `shiplog/in-progress` | Lifecycle — work started | Branch exists, session started |
| `shiplog/needs-review` | Lifecycle — PR awaits review | PR opened against the issue |
| `shiplog/verification` | Gate — extra verification needed | Added manually when warranted |

**Lifecycle labels are mutually exclusive.** An issue may have at most one of
`shiplog/ready`, `shiplog/in-progress`, `shiplog/needs-review` at any time.
The kind label (`shiplog/plan`) and gate labels (`shiplog/verification`)
coexist freely with lifecycle labels.

## When to update labels

| Event | Label change |
|-------|-------------|
| Issue created with `readiness=ready` | Add `shiplog/ready` |
| Issue created with `readiness=backlog` | No lifecycle label (just `shiplog/plan`) |
| `shiplog:start` (branch created) | Remove `shiplog/ready`, add `shiplog/in-progress` |
| PR opened | Remove `shiplog/in-progress`, add `shiplog/needs-review` |
| PR merged / issue closed | Remove all lifecycle labels |
| Envelope readiness edited | Update lifecycle label to match |

## Audit and repair

When label drift is suspected, run this one-liner to compare envelope
readiness against current labels:

```bash
gh api graphql -f query='{
  repository(owner: "galeon-engine", name: "galeon") {
    issues(states: OPEN, first: 100) {
      nodes {
        number title body
        labels(first: 10) { nodes { name } }
      }
    }
  }
}' --jq '
  .data.repository.issues.nodes[]
  | select(.body | test("shiplog:envelope"))
  | {
      number,
      labels: [.labels.nodes[].name],
      readiness: (
        if (.body | test("readiness="))
        then (.body | capture("readiness=(?<r>[^ ]+)") | .r)
        else "unset"
        end
      )
    }
'
```

Compare `readiness` to the lifecycle label present. Fix mismatches with:

```bash
# Example: envelope says ready, label missing
gh issue edit <N> --repo galeon-engine/galeon --add-label "shiplog/ready"

# Example: wrong lifecycle label (must remove old first)
gh issue edit <N> --repo galeon-engine/galeon \
  --remove-label "shiplog/ready" \
  --add-label "shiplog/in-progress"
```

## Preventing drift

Label drift is a manual-discipline problem. The rule:

> **Whoever changes an envelope's `readiness` field also updates the lifecycle label in the same action.**

This applies to both humans and agents. The shiplog `start`, `commit`, and
`pr` commands already transition labels as part of their flow. Drift happens
when envelopes are edited outside those commands (e.g., bulk-creating issues
with `readiness=ready` but forgetting the label).

No automation is added at this time. The audit query above is cheap to run
and catches drift quickly. If drift becomes a recurring problem, a GitHub
Action that parses envelopes on issue edit events would be the next step.
