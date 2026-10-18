#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
fixture_dir="$(mktemp -d /private/tmp/hindsight-remote-test.XXXXXX)"
server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi
  rm -f "$fixture_dir/key.pem" "$fixture_dir/cert.pem" "$fixture_dir/identity.p12" "$fixture_dir/smoke" "$fixture_dir/server.log"
  rmdir "$fixture_dir" 2>/dev/null || true
}
trap cleanup EXIT

openssl req -x509 -newkey rsa:2048 -nodes -keyout "$fixture_dir/key.pem" \
  -out "$fixture_dir/cert.pem" -days 1 -subj /CN=localhost \
  -addext subjectAltName=IP:127.0.0.1,DNS:localhost >/dev/null 2>&1
openssl pkcs12 -export -inkey "$fixture_dir/key.pem" -in "$fixture_dir/cert.pem" \
  -out "$fixture_dir/identity.p12" -passout pass:test-password
CLANG_MODULE_CACHE_PATH=/private/tmp/hindsight-remote-module-cache swiftc -emit-executable -o "$fixture_dir/smoke" \
  "$project_root/App/RemoteGateway.swift" "$project_root/Tests/RemoteGatewaySmoke.swift"

port="${HINDSIGHT_GATEWAY_TEST_PORT:-18449}"
"$fixture_dir/smoke" "$fixture_dir/identity.p12" "$port" test-password >"$fixture_dir/server.log" 2>&1 &
server_pid=$!
url="https://127.0.0.1:$port/v1/capabilities"
token="$(printf 'a%.0s' {1..64})"
ready=0
for _ in {1..30}; do
  if curl --silent --cacert "$fixture_dir/cert.pem" -H "Authorization: Bearer $token" \
      -o /dev/null "$url"; then ready=1; break; fi
  sleep 0.1
done
if [[ "$ready" != 1 ]]; then
  sed -n '1,30p' "$fixture_dir/server.log"
  exit 1
fi

request_code() {
  curl --silent --show-error --cacert "$fixture_dir/cert.pem" -o /dev/null -w '%{http_code}' "$@"
}
[[ "$(request_code -H "Authorization: Bearer $token" "$url")" == 200 ]]
[[ "$(request_code "$url")" == 401 ]]
[[ "$(request_code -H "Authorization: Bearer $token" -H 'Host: evil.invalid' "$url")" == 400 ]]
[[ "$(request_code -H "Authorization: Bearer $token" -H 'Origin: https://evil.example' "$url")" == 400 ]]
[[ "$(request_code -X OPTIONS -H 'Origin: https://client.example' \
    -H 'Access-Control-Request-Method: POST' \
    -H 'Access-Control-Request-Headers: authorization, content-type' "$url")" == 204 ]]
[[ "$(request_code -X OPTIONS -H 'Origin: https://evil.example' \
    -H 'Access-Control-Request-Method: POST' "$url")" == 400 ]]
cors_headers="$(curl --silent --show-error --cacert "$fixture_dir/cert.pem" -D - -o /dev/null \
  -H "Authorization: Bearer $token" -H 'Origin: https://client.example' "$url")"
[[ "$cors_headers" == *$'Access-Control-Allow-Origin: https://client.example\r'* ]]
[[ "$(request_code -H "Authorization: Bearer $token" -H 'Content-Type: application/json' \
    -d '{}' "https://127.0.0.1:$port/v1/jobs/analysis")" == 202 ]]
echo "Remote gateway TLS/auth/host/body smoke passed."
