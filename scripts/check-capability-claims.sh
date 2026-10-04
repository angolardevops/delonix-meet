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
#  WHAT COUNTS AS A CLAIM (changed on 2026-10-04)
#
#  The original rule above read `web/src/locales/*.ts` and looked for roadmap
#  entries marked `done: true` and pricing `features:` lists. Both assumptions
#  rotted: the locale files moved one level down (`locales/<lang>/*.ts`), so
#  the glob matched nothing and `2>/dev/null` hid it; and the pricing page and
#  the roadmap left the locales altogether. For weeks this gate was green
#  because it was reading zero files.
#
#  The claim surface today is the console's own copy. So: a guarded term that
#  appears in ANY locale string is a claim, and needs implementing code outside
#  the locale files. The old markers are still honoured if they ever return.
#  The lighting screen was the live case — it named "Art-Net · sACN · Philips
#  Hue" under a "no agent" badge, with no line of code speaking any of them.
#
#  To say honestly that something is NOT available, say it without the product
#  name ("no lighting agent"), or build it.
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

# Files that name third-party products without claiming to integrate them. The
# diagram editor's shape catalogue lets you DRAW an "Amazon S3" box; that is a
# label on a shape, not a storage backend.
EXEMPT_RE='/diagramCatalog\.ts$'

mapfile -t LOCALES < <(find web/src/locales -mindepth 2 -type f -name '*.ts' 2>/dev/null \
                        | grep -vE "$EXEMPT_RE" | sort)
langs=$(find web/src/locales -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l)

# A gate that reads nothing must fail, not pass. This is the defect that kept
# it green from the day the locales were split by language.
if [ "${#LOCALES[@]}" -eq 0 ] || [ "$langs" -lt 2 ]; then
  echo "✗ claims: no locale files found under web/src/locales/<lang>/ — the gate would read nothing."
  echo "     Found ${#LOCALES[@]} file(s) in $langs language dir(s). Fix the path here; do not let it pass empty."
  exit 1
fi

# term | regex proving implementing code exists somewhere that is not a locale.
# The proof is deliberately code-shaped (identifiers, crate names), so that a
# comment or a constant string naming the product does not satisfy it: on
# 2026-10-04 a fixed `kind: "minio"` in a JSON response was passing for an
# object-storage client that does not exist.
GUARDED=(
  'SAML|saml'
  'SCIM|scim'
  'LDAP|ldap3|ldap_bind'
  'WebAuthn|webauthn|PublicKeyCredential'
  'passkey|passkey'
  'SVC|scalabilityMode'
  'SDK|sdk'
  'webinar|webinar'
  'MinIO|aws_sdk_s3|s3_client|object_store::'
  'S3|aws_sdk_s3|s3_client|object_store::'
  'HLS|m3u8'
  'NDI|NDIlib|ndi_send'
  'WHIP|whip_endpoint|/whip'
  'SIPREC|siprec'
  'MLS|openmls'
  'DMX|dmx512|dmxUniverse|dmx_universe'
  'Art-Net|artnet|art_net|ArtNetPacket'
  'sACN|sacn|e131'
  'Philips Hue|hueBridge|hue_bridge|philips_hue'
)

for entry in "${GUARDED[@]}"; do
  term=${entry%%|*}
  proof=${entry#*|}

  # Any locale string naming the capability is a claim (see the header).
  claims=$(grep -nE "\b${term}\b" "${LOCALES[@]}" || true)
  [ -z "$claims" ] && continue

  # Proof: the term's implementation, anywhere that is not a locale file.
  if ! grep -rqE "$proof" server/src server/crates web/src \
        --include='*.rs' --include='*.ts' --include='*.tsx' \
        --exclude-dir=locales 2>/dev/null; then
    echo "✗ claims: '${term}' is named in the product's copy, but no implementing code exists."
    echo "$claims" | sed 's/^/     /' | cut -c1-160
    echo "     Either build it, or take the name off the screen."
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
quantified=$(grep -nE "[0-9]{2}[.,][0-9]+ ?%" "${LOCALES[@]}" \
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

[ "$fail" = 0 ] && echo "✓ capability claims: ${#LOCALES[@]} locale files in $langs languages read; nothing is named without code behind it"
exit $fail
