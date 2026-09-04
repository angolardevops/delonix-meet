#!/usr/bin/env bash
# ============================================================
#  Fitness function: a capability may not be SOLD before it is BUILT.
#
#  Written in English, unlike its siblings in this directory, under the
#  boundary rule agreed on 2026-09-03: new files, identifiers and public
#  surfaces in English; existing internal code and the harness stay as they
#  are. If the mixed output across `make fitness` proves more annoying than
#  the rule is worth, this is the file to convert back.
#
#  WHY IT EXISTS
#
#  On 2026-09-03 the pricing page was selling "SAML SSO and SCIM" on a paid
#  tier, in three languages. Neither had a single line of implementing code —
#  the only occurrences of both words in the whole tree were the marketing
#  strings themselves. Four roadmap entries carried `done: true` for things
#  that did not exist either: SVC, server-side bandwidth estimation, verifiable
#  E2EE security codes, and a public SDK.
#
#  None of that was dishonesty. It is what happens when copy is written to the
#  roadmap instead of to the tree, and nothing ever re-reads it. A gate does
#  the re-reading.
#
#  WHAT IT CHECKS
#
#  For each guarded term below: if the term appears in a locale file on a line
#  that claims the capability SHIPPED — a roadmap entry marked `done: true`, or
#  a pricing tier's `features:` list — then implementing code must exist
#  outside the locale files. No code, no claim.
#
#  A term on a roadmap line WITHOUT `done: true` is a plan, not a claim, and
#  passes untouched. Promising something is fine. Reporting it as delivered is
#  what this refuses.
#
#  HONEST LIMIT
#
#  This proves a capability is not claimed with ZERO code behind it. It cannot
#  prove the code is complete, reachable, or authorised — a stub named right
#  would satisfy it. It catches the failure that actually happened, which is
#  the claim with nothing at all behind it.
#
#  Usage:  bash scripts/check-capability-claims.sh
# ============================================================
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0

LOCALES=(web/src/locales/*.ts)

# term | regex proving implementing code exists somewhere that is not a locale
GUARDED=(
  'SAML|saml'
  'SCIM|scim'
  'WebAuthn|webauthn|PublicKeyCredential'
  'passkey|passkey'
  'SVC|scalabilityMode'
  'SDK|sdk'
  'webinar|webinar'
  'MinIO|minio|aws_sdk_s3|s3_client'
)

for entry in "${GUARDED[@]}"; do
  term=${entry%%|*}
  proof=${entry#*|}

  # Lines that claim the capability is DELIVERED, in any locale.
  claims=$(grep -nE "\b${term}\b" "${LOCALES[@]}" 2>/dev/null \
           | grep -E 'done: true|features:' || true)
  [ -z "$claims" ] && continue

  # Proof: the term's implementation, anywhere that is not a locale file.
  if ! grep -rqE "$proof" server/src web/src \
        --include='*.rs' --include='*.ts' --include='*.tsx' \
        --exclude-dir=locales 2>/dev/null; then
    echo "✗ claims: '${term}' is sold as delivered, but no implementing code exists."
    echo "$claims" | sed 's/^/     /' | cut -c1-160
    echo "     Either build it, or move it to a roadmap entry without 'done: true'."
    fail=1
  fi
done

# A QUANTIFIED availability guarantee is a different animal from a capability,
# and needs a different rule. "SLA agreed by contract" is a commercial term and
# says nothing about the software. "99.99% SLA" is a number the platform has to
# be able to hold and prove, and on 2026-09-03 it was printed on a paid tier by
# a platform with no SLO, no error budget, no load test and no chaos result.
#
# The first version of this gate guarded the bare word "SLA" and therefore also
# refused the harmless contractual phrasing. Guard the NUMBER instead: that is
# the part that requires evidence.
quantified=$(grep -nE "[0-9]{2}[.,][0-9]+ ?%" "${LOCALES[@]}" 2>/dev/null \
             | grep -iE 'sla|uptime|availability|disponibilidade|disponibilité' || true)
if [ -n "$quantified" ]; then
  if ! grep -rqE 'error_budget|slo_target|availability_target' server/src \
        --include='*.rs' 2>/dev/null; then
    echo "✗ claims: a NUMERIC availability guarantee is published, with nothing measuring it."
    echo "$quantified" | sed 's/^/     /' | cut -c1-160
    echo "     A percentage is a promise. Publish it once an error budget exists,"
    echo "     or state the SLA as contractual instead of numeric."
    fail=1
  fi
fi

[ "$fail" = 0 ] && echo "✓ capability claims: nothing is sold as delivered without code behind it"
exit $fail
