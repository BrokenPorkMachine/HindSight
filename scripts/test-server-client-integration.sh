#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
[[ "$(uname -s)" == Darwin ]] || { echo "Client integration requires macOS." >&2; exit 1; }
fixture_dir="$(mktemp -d /private/tmp/hindsight-remote-e2e.XXXXXX)"
app_pid=""
cleanup() {
  if [[ -n "$app_pid" ]]; then kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true; fi
  if [[ "$fixture_dir" == /private/tmp/hindsight-remote-e2e.* ]]; then rm -rf -- "$fixture_dir"; fi
}
trap cleanup EXIT

port="${HINDSIGHT_REMOTE_E2E_PORT:-18450}"
token="$(openssl rand -hex 32)"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$fixture_dir/key.pem" \
  -out "$fixture_dir/cert.pem" -days 1 -subj /CN=localhost \
  -addext subjectAltName=IP:127.0.0.1,DNS:localhost >/dev/null 2>&1
openssl pkcs12 -export -inkey "$fixture_dir/key.pem" -in "$fixture_dir/cert.pem" \
  -out "$fixture_dir/identity.p12" -passout pass:test-password

echo "Building test-only app integration hook and native worker"
xcodebuild -quiet -project "$project_root/HindSight.xcodeproj" -scheme HindSight \
  -configuration Debug -derivedDataPath /private/tmp/hindsight-server-e2e-build \
  SWIFT_ACTIVE_COMPILATION_CONDITIONS=HINDSIGHT_SERVER_TEST \
  build CODE_SIGNING_ALLOWED=NO
app="/private/tmp/hindsight-server-e2e-build/Build/Products/Debug/HindSight.app"
[[ -x "$app/Contents/MacOS/HindSight" ]] || { echo "App executable missing." >&2; exit 1; }

HINDSIGHT_REMOTE_INTEGRATION_SMOKE=1 \
HINDSIGHT_REMOTE_SMOKE_PROJECT="$fixture_dir/project" \
HINDSIGHT_REMOTE_SMOKE_BINARY="$project_root/Tests/Fixtures/arm64-integer-sample.macho" \
HINDSIGHT_REMOTE_SMOKE_IDENTITY="$fixture_dir/identity.p12" \
HINDSIGHT_REMOTE_SMOKE_PASSWORD=test-password \
HINDSIGHT_REMOTE_SMOKE_TOKEN="$token" \
HINDSIGHT_REMOTE_SMOKE_PORT="$port" \
  "$app/Contents/MacOS/HindSight" >"$fixture_dir/app.log" 2>&1 &
app_pid=$!

base="https://127.0.0.1:$port"
project_json=""
for _ in {1..120}; do
  project_json="$(curl --silent --cacert "$fixture_dir/cert.pem" \
    -H "Authorization: Bearer $token" "$base/v1/project" || true)"
  if jq -e '.records[0].result.captured == true' >/dev/null 2>&1 <<<"$project_json"; then break; fi
  if ! kill -0 "$app_pid" 2>/dev/null; then sed -n '1,90p' "$fixture_dir/app.log"; exit 1; fi
  sleep 0.5
done
if ! jq -e '.records[0].result.captured == true' >/dev/null 2>&1 <<<"$project_json"; then
  echo "App did not expose the imported project snapshot." >&2
  sed -n '1,90p' "$fixture_dir/app.log" >&2
  exit 1
fi
schema="$(curl --silent --show-error --cacert "$fixture_dir/cert.pem" \
  -H "Authorization: Bearer $token" "$base/v1/openapi.json")"
if ! jq -e '.openapi == "3.1.0" and (.paths | has("/jobs/analysis")) and (.paths | has("/jobs/{job_id}/result"))' \
  >/dev/null <<<"$schema"; then
  echo "Served OpenAPI contract failed client validation:" >&2
  jq . <<<"$schema" >&2 || true
  exit 1
fi
capabilities="$(curl --silent --show-error --cacert "$fixture_dir/cert.pem" \
  -H "Authorization: Bearer $token" "$base/v1/capabilities")"
if ! jq -e '.protocol == 1 and (.features | index("analysis_jobs")) != null and .execution == false' \
  >/dev/null <<<"$capabilities"; then
  echo "Served capability response failed client validation: $capabilities" >&2
  exit 1
fi
record_id="$(jq -er '.records[0].id' <<<"$project_json")"
digest="$(jq -er '.records[0].result.source_sha256' <<<"$project_json")"
project_path="$(jq -er '.path' <<<"$project_json")"
request_id="$(uuidgen)"
body="$(jq -nc --arg request_id "$request_id" --arg project_path "$project_path" \
  --arg record_id "$record_id" --arg source_sha256 "$digest" \
  '{request_id:$request_id,project_path:$project_path,record_id:$record_id,source_sha256:$source_sha256}')"

post() {
  curl --silent --show-error --cacert "$fixture_dir/cert.pem" \
    -H "Authorization: Bearer $token" -H 'Content-Type: application/json' \
    -d "$body" "$base/v1/jobs/analysis"
}
accepted="$(post)"
jq -e --arg id "$request_id" '.job_id == $id and .state == "accepted"' >/dev/null <<<"$accepted"
retried="$(post)"
jq -e --arg id "$request_id" '.job_id == $id and .state == "existing"' >/dev/null <<<"$retried"

result=""
for _ in {1..120}; do
  result="$(curl --silent --cacert "$fixture_dir/cert.pem" \
    -H "Authorization: Bearer $token" "$base/v1/jobs/$request_id/result" || true)"
  if jq -e --arg id "$request_id" --arg digest "$digest" \
      '.ok == true and .analysis.request_id == $id and .analysis.source_sha256 == $digest' \
      >/dev/null 2>&1 <<<"$result"; then
    echo "Real app → XPC coordinator → worker → HTTPS client transaction passed."
    exit 0
  fi
  if ! kill -0 "$app_pid" 2>/dev/null; then sed -n '1,90p' "$fixture_dir/app.log"; exit 1; fi
  sleep 0.5
done
echo "Timed out waiting for source-bound analysis result." >&2
sed -n '1,90p' "$fixture_dir/app.log" >&2
exit 1
