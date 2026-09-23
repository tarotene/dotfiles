#!/usr/bin/env bash
# _rulesets-apply-core.sh — shared core for the rust/typst/astro-site-
# repo-governance skills' apply-rulesets.sh (#388). Sourced, never executed
# directly (no shebang execute bit, no #!/usr/bin/env bash dispatch below).
#
# apply-rulesets.sh differs from setup-hooks.sh/apply-repo-settings.sh
# (also de-duplicated by #388, see those files): it is NOT a byte-for-byte
# duplicate across the three skills. Each skill substitutes real
# ecosystem-specific placeholders (rust: __MSRV__/__CANONICAL_CRATE__/...;
# astro: __NODE_VERSION__/__PACKAGE_NAME__/...; typst: __TYPST_VERSION__/
# __MIN_TYPST__/...) into its own quality.json template, so that logic
# genuinely differs per skill and stays in each skill's own
# apply-rulesets.sh. What IS identical across all three is everything
# downstream of the substitution: removing the review layer, and the
# create-only POST loop over security/quality/workflow(+review).json. This
# file holds exactly that identical part.
#
# Contract: before sourcing this file, the caller must set OWNER, REPO,
# DRY_RUN, WITH_REVIEW, REMOVE_REVIEW, and define a `process_ruleset <file>`
# function that prints the placeholder-substituted ruleset JSON to stdout.
# After sourcing, the caller calls `governance_apply_rulesets_main
# "$RULESETS_DIR"`. On the remove-review path this function exits the
# whole script (matching the pre-#388 behavior exactly — remove-review was
# always a terminal path, never falling through to the create-only loop or
# to a skill's own trailing NOTE text); otherwise it returns normally so
# the caller can print its own ecosystem-specific trailing note before this
# file's own review-layer opt-in reminder.

remove_review_layer() {
  echo "Removing review layer from: $OWNER/$REPO"
  local rulesets review_id ids id detail has_review new_body name

  rulesets=$(gh api "repos/$OWNER/$REPO/rulesets" 2>/dev/null || echo '[]')

  review_id=$(jq -r '.[] | select(.name=="Review") | .id' <<<"$rulesets" | head -1)
  if [[ -n "$review_id" ]]; then
    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would DELETE Ruleset 'Review' (id=$review_id)"
    else
      gh api -X DELETE "repos/$OWNER/$REPO/rulesets/$review_id" >/dev/null
      echo "  ✓  Deleted Ruleset 'Review' (id=$review_id)"
    fi
  fi

  # Exclude review_id — it was just deleted above (if it existed), so a
  # second GET/PUT round trip on the same id would 404.
  ids=$(jq -r --arg rid "$review_id" '.[] | select(.target=="branch" and .enforcement=="active" and (.id|tostring) != $rid) | .id' <<<"$rulesets")
  while IFS= read -r id; do
    [[ -n "$id" ]] || continue
    detail=$(gh api "repos/$OWNER/$REPO/rulesets/$id" 2>/dev/null) || continue
    [[ -n "$detail" ]] || continue

    has_review=false
    jq -e '
      ([.rules[]?.type] | index("copilot_code_review"))
      or (any(.rules[]?; .type=="pull_request" and (.parameters.required_review_thread_resolution // false) == true))
    ' <<<"$detail" >/dev/null 2>&1 && has_review=true
    [[ "$has_review" == "true" ]] || continue

    name=$(jq -r '.name' <<<"$detail")
    new_body=$(jq '
      {name, target, enforcement, conditions, bypass_actors,
       rules: [.rules[] | select(.type != "copilot_code_review")
               | if .type == "pull_request"
                 then .parameters.required_review_thread_resolution = false
                 else . end]}
    ' <<<"$detail")

    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would PUT Ruleset '$name' (id=$id) stripped of the review layer:"
      echo "$new_body" | jq .
    else
      echo "$new_body" | gh api -X PUT "repos/$OWNER/$REPO/rulesets/$id" --input - >/dev/null
      echo "  ✓  Updated Ruleset '$name' (id=$id) — review layer removed"
    fi
  done <<<"$ids"
}

governance_apply_rulesets_main() {
  local rulesets_dir="$1"

  if [[ "$REMOVE_REVIEW" == "true" ]]; then
    remove_review_layer
    exit 0
  fi

  echo "Applying Rulesets to: $OWNER/$REPO"
  echo ""

  local EXISTING_NAMES
  EXISTING_NAMES=$(gh api "repos/$OWNER/$REPO/rulesets" --jq '.[].name' 2>/dev/null || echo "")

  local RULESET_FILES=(
    "$rulesets_dir/security.json"
    "$rulesets_dir/quality.json"
    "$rulesets_dir/workflow.json"
  )
  [[ "$WITH_REVIEW" == "true" ]] && RULESET_FILES+=("$rulesets_dir/review.json")

  local ruleset_file name processed result id
  for ruleset_file in "${RULESET_FILES[@]}"; do
    name=$(jq -r '.name' "$ruleset_file")
    processed=$(process_ruleset "$ruleset_file")

    if ! echo "$processed" | jq -e . >/dev/null 2>&1; then
      echo "  ERROR: Invalid JSON after substitution for '$name' — aborting."
      exit 1
    fi

    if echo "$EXISTING_NAMES" | grep -qF "$name"; then
      echo "  ⚠   '$name' already exists — skipping."
      echo "      To update: gh api repos/$OWNER/$REPO/rulesets/<id> -X PUT --input <file>"
      continue
    fi

    if [[ "$DRY_RUN" == "true" ]]; then
      echo "  DRY-RUN: would POST Ruleset '$name':"
      echo "$processed" | jq .
      echo ""
    else
      result=$(echo "$processed" | gh api -X POST "repos/$OWNER/$REPO/rulesets" --input -)
      id=$(echo "$result" | jq -r '.id')
      echo "  ✓  Created Ruleset '$name' (id=$id)"
    fi
  done

  echo ""
  echo "NOTE: Required status check contexts in quality.json must exactly match"
  echo "the 'name:' fields of the corresponding CI workflow jobs — update both"
  echo "together when renaming a job."
  if [[ "$WITH_REVIEW" != "true" ]]; then
    echo "NOTE: Review layer (Copilot code review + required conversation resolution)"
    echo "was not applied — pass --with-review to opt in once this repository is past"
    echo "its early-development phase (ADR-0021 in tarotene/dotfiles)."
  fi
}
