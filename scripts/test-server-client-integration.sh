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
xcrun clang -target arm64-apple-macos14.0 -dynamiclib -Wl,-no_fixup_chains \
  "$project_root/Tests/Fixtures/Decompilation/batch_review.s" -o "$fixture_dir/batch.dylib"
xcodebuild -quiet -project "$project_root/HindSight.xcodeproj" -scheme HindSight \
  -configuration Debug -derivedDataPath /private/tmp/hindsight-server-e2e-build \
  SWIFT_ACTIVE_COMPILATION_CONDITIONS=HINDSIGHT_SERVER_TEST \
  build CODE_SIGNING_ALLOWED=NO
app="/private/tmp/hindsight-server-e2e-build/Build/Products/Debug/HindSight.app"
[[ -x "$app/Contents/MacOS/HindSight" ]] || { echo "App executable missing." >&2; exit 1; }

HINDSIGHT_REMOTE_INTEGRATION_SMOKE=1 \
HINDSIGHT_REMOTE_SMOKE_PROJECT="$fixture_dir/project" \
HINDSIGHT_REMOTE_SMOKE_BINARY="$fixture_dir/batch.dylib" \
HINDSIGHT_REMOTE_SMOKE_IDENTITY="$fixture_dir/identity.p12" \
HINDSIGHT_REMOTE_SMOKE_PASSWORD=test-password \
HINDSIGHT_REMOTE_SMOKE_TOKEN="$token" \
HINDSIGHT_REMOTE_SMOKE_PORT="$port" \
HINDSIGHT_RECONSTRUCTION_SMOKE_OUTPUT="$fixture_dir/reconstruction.json" \
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
if ! jq -e --arg digest "$(jq -er '.records[0].result.source_sha256' <<<"$project_json")" \
    '.schema == 1 and .producer == "hindsight-reconstruction-v1" and .source_sha256 == $digest and .function.start != .function.end and (.nodes | length) > 0 and (.ast.compilable == false)' \
    "$fixture_dir/reconstruction.json" >/dev/null 2>&1; then
  echo "Real app → XPC → worker → app reconstruction review transaction failed." >&2
  sed -n '1,90p' "$fixture_dir/app.log" >&2
  exit 1
fi
python3 - "$project_root" "$fixture_dir/reconstruction.json.batch" <<'PY'
import hashlib
import json
from pathlib import Path
import sys
import subprocess
sys.path.insert(0, str(Path(sys.argv[1]) / "scripts"))
from reconstruction_artifact import ReconstructionArtifact
root = Path(sys.argv[2])
def check_outcome(value):
    states = [entry["state"] for entry in value["functions"]]
    for state, key in [("recovered", "recovered_functions"), ("review-only", "review_only_functions"), ("failed", "failed_functions")]:
        assert value[key] == states.count(state)
    expected = ("cancelled" if value["cancelled"] else "incomplete" if len(states) < len(value["planned"]) else
                "completed-with-failures" if "failed" in states else "completed")
    assert value["batch_status"] == expected
manifest = json.loads((root / "manifest.json").read_text())
check_outcome(manifest)
assert manifest["attempted"] == manifest["discovered"] > 0
assert manifest["attempted"] >= 3, "fixture must exercise sequential multi-function reconstruction"
assert manifest["cancelled"] is False
total = 0
for entry in manifest["functions"]:
    assert entry["state"] in {"review-only", "recovered"}
    assert entry["files"]
    for exported in entry["files"]:
        assert Path(exported["path"]).name == exported["path"]
        raw = (root / exported["path"]).read_bytes()
        assert len(raw) == exported["bytes"]
        assert hashlib.sha256(raw).hexdigest() == exported["sha256"]
        total += len(raw)
        if exported["path"].endswith(".reconstruction.json"):
            document = json.loads(raw)
            ReconstructionArtifact(document)
            assert document["source_sha256"] == manifest["source_sha256"]
            assert int(document["function"]["start"], 16) == int(entry["start"], 16)
            assert int(document["function"]["end"], 16) == int(entry["end"], 16)
assert total == manifest["output_bytes"]
for format_name, extension, kind in [("c", "c", "recovered-source"), ("objc", "m", "recovered-source"),
                                    ("header", "h", "recovered-header"), ("native", "n", "machine-state-c")]:
    dump_root = root / ("dump-" + extension)
    dumped = json.loads((dump_root / "export-manifest.json").read_text())
    assert dumped["format"] == format_name and dumped["artifact_kind"] == kind
    assert dumped["source_sha256"] == manifest["source_sha256"]
    assert dumped["available"] + dumped["unavailable"] == len(manifest["functions"])
    assert dumped["available"] == len(manifest["functions"]), "fixture dump must contain code files, not just JSON"
    assert len(dumped["functions"]) == len(manifest["functions"])
    available = total_dump_bytes = 0
    for entry in dumped["functions"]:
        artifacts = [item for item in entry["files"] if item["path"].endswith(".reconstruction.json")]
        assert len(artifacts) == 1
        artifact = json.loads((dump_root / artifacts[0]["path"]).read_bytes())
        ReconstructionArtifact(artifact)
        candidates = [node["payload"]["source"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == kind]
        actual_kind = kind
        if not candidates and format_name in {"c", "objc"}:
            actual_kind = "machine-state-c"
            candidates = [node["payload"]["source"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == actual_kind]
        if not candidates:
            actual_kind = "best-effort-header" if kind == "recovered-header" else "best-effort-source"
            candidates = [node["payload"]["source"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == actual_kind]
        assert len(candidates) <= 1 and entry["available"] == bool(candidates)
        assert entry["representation"] == (actual_kind if candidates else "unavailable")
        if kind == "recovered-header":
            header_payloads = [node["payload"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == actual_kind]
            assert entry["languages"] == (header_payloads[0]["languages"] if header_payloads else [])
        available += bool(candidates)
        for item in entry["files"]:
            assert Path(item["path"]).name == item["path"]
            raw = (dump_root / item["path"]).read_bytes()
            if item["path"].endswith("." + extension) and entry["representation"] in {"machine-state-c", "recovered-source"}:
                subprocess.run(["xcrun", "clang", "-fsyntax-only", "-x", "objective-c" if format_name == "objc" else "c",
                                "-std=c17", str(dump_root / item["path"])], check=True)
            assert len(raw) == item["bytes"] and hashlib.sha256(raw).hexdigest() == item["sha256"]
            total_dump_bytes += len(raw)
            if not item["path"].endswith(".json"):
                assert item["path"].endswith("." + extension) and raw.decode() == candidates[0]
    assert available == dumped["available"] and total_dump_bytes == dumped["output_bytes"]
print("App format dumps passed: .c/.m/.h/.n availability, artifact identity and exact output hashes verified.")
all_root = root / "dump-all"
all_manifest = json.loads((all_root / "export-manifest.json").read_bytes())
assert all_manifest["kind"] == "reconstruction-all-formats-export"
assert [item["folder"] for item in all_manifest["formats"]] == ["c", "h", "m", "n"]
for item in all_manifest["formats"]:
    folder = item["folder"]
    raw = (all_root / folder / "export-manifest.json").read_bytes()
    assert hashlib.sha256(raw).hexdigest() == item["manifest_sha256"]
    assert raw == (root / ("dump-" + folder) / "export-manifest.json").read_bytes()
    for function in json.loads(raw)["functions"]:
        for file in function["files"]:
            dumped = (all_root / folder / file["path"]).read_bytes()
            assert len(dumped) == file["bytes"] and hashlib.sha256(dumped).hexdigest() == file["sha256"]
print("App dump-all passed: c/h/m/n folders match individual dumps byte-for-byte.")
failed_root = root.with_name(root.name + "-failed")
failed = json.loads((failed_root / "manifest.json").read_bytes())
check_outcome(failed)
assert failed["batch_status"] == "completed-with-failures" and failed["failed_functions"] == failed["attempted"] == 1
assert failed["functions"][0]["state"] == "failed" and not failed["functions"][0].get("files")
assert not list(failed_root.glob("function-*")), "rejected source identity published function output"
failed_dump = json.loads((failed_root / "dump-h/export-manifest.json").read_bytes())
assert failed_dump["available"] == 0 and failed_dump["unavailable"] == failed_dump["failed_functions"] == 1
assert failed_dump["functions"][0]["state"] == "failed" and failed_dump["functions"][0]["files"] == []
print("App rejected stale-identity worker output and preserved explicit batch/dump failure counts.")
for scenario, cancelled in [("cancelled", True), ("stale-context", False)]:
    stopped_root = root.with_name(root.name + "-" + scenario)
    stopped = json.loads((stopped_root / "manifest.json").read_text())
    check_outcome(stopped)
    assert stopped["attempted"] == 0 and stopped["functions"] == []
    assert stopped["cancelled"] is cancelled
    assert {path.name for path in stopped_root.iterdir()} == {"manifest.json", "checkpoint.json"}
resumed_root = root.with_name(root.name + "-resumed")
resumed = json.loads((resumed_root / "manifest.json").read_text())
check_outcome(resumed)
assert resumed["attempted"] == manifest["attempted"]
assert resumed["cancelled"] is False
for entry in resumed["functions"]:
    assert entry["state"] in {"review-only", "recovered"}
    for exported in entry["files"]:
        raw = (resumed_root / exported["path"]).read_bytes()
        assert len(raw) == exported["bytes"] and hashlib.sha256(raw).hexdigest() == exported["sha256"]
print(f"App/XPC reconstruction batch passed: {manifest['attempted']} functions; exported hashes verified.")
partial_root = root.with_name(root.name + "-partial")
partial = json.loads((partial_root / "manifest.json").read_text())
check_outcome(partial)
assert partial["attempted"] == manifest["attempted"] and partial["retained"] == 1
assert partial["cancelled"] is False
assert partial["requested_format"] == "native" and partial["scope"] == "whole"
for entry in partial["functions"]:
    assert entry["state"] in {"review-only", "recovered"}
    for exported in entry["files"]:
        raw = (partial_root / exported["path"]).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == exported["sha256"]
resumed_dump_root = partial_root / "dump-n"
resumed_dump = json.loads((resumed_dump_root / "export-manifest.json").read_text())
assert resumed_dump["format"] == "native" and resumed_dump["available"] == len(partial["functions"])
assert resumed_dump["unavailable"] == 0
assert resumed_dump["source_checkpoint_sha256"] == hashlib.sha256((partial_root / "checkpoint.json").read_bytes()).hexdigest()
for entry in resumed_dump["functions"]:
    artifacts = [file for file in entry["files"] if file["path"].endswith(".reconstruction.json")]
    assert len(artifacts) == 1
    artifact = json.loads((resumed_dump_root / artifacts[0]["path"]).read_bytes())
    ReconstructionArtifact(artifact)
    native = [node["payload"]["source"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == "machine-state-c"]
    assert len(native) == 1
    for file in entry["files"]:
        raw = (resumed_dump_root / file["path"]).read_bytes()
        assert len(raw) == file["bytes"] and hashlib.sha256(raw).hexdigest() == file["sha256"]
        if file["path"].endswith(".n"):
            assert raw.decode() == native[0]
print("App partial batch cancellation/resume passed; completed files preserved.")
print("Requested native dump intent survived cancellation/resume; output identity and hashes verified.")
PY

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
    echo "Real app → XPC → worker → native reconstruction decoder and HTTPS client transactions passed."
    exit 0
  fi
  if ! kill -0 "$app_pid" 2>/dev/null; then sed -n '1,90p' "$fixture_dir/app.log"; exit 1; fi
  sleep 0.5
done
echo "Timed out waiting for source-bound analysis result." >&2
sed -n '1,90p' "$fixture_dir/app.log" >&2
exit 1
