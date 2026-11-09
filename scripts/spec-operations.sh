#!/usr/bin/env bash
# Print the operations of the live Henrri v1 specification, in the format of
# spec/operations.tsv, so CI (and you) can see whether the API moved.
#
#   scripts/spec-operations.sh            # production
#   scripts/spec-operations.sh sandbox    # sandbox
set -euo pipefail
case "${1:-production}" in
  production) host=api.henrri.io ;;
  sandbox) host=api-sandbox.henrri.io ;;
  *) echo "usage: $0 [production|sandbox]" >&2; exit 2 ;;
esac
spec=$(curl -fsSL "https://$host/henrriapi/v1.json")
echo "# Every operation of the Henrri v1 specification, as published at"
echo "# https://api.henrri.io/henrriapi/v1.json and https://api-sandbox.henrri.io/henrriapi/v1.json."
echo "# Regenerate with scripts/spec-operations.sh; CI fails when it drifts from the live spec."
jq -r '.paths | to_entries[] | .key as $p | .value | to_entries[]
  | select(.key | test("^(get|post|put|patch|delete)$"))
  | "\(.key | ascii_upcase)\t\($p)\t\(.value.operationId)"' <<<"$spec" | sort -k2,2 -k1,1
jq -r '.webhooks // {} | keys[] | "# webhook\t\(.)"' <<<"$spec"
