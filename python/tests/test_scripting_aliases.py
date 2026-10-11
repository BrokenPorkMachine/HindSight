import json
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ScriptingAliasTests(unittest.TestCase):
    def test_new_and_legacy_extensions_share_the_same_api(self):
        source = (ROOT / "PlatformServices/CoordinatorService/ScriptingHost.swift").read_text()
        runner = source.split('private let runner = #"""', 1)[1].split('"""#', 1)[0]
        request = {
            "source": "import hindsight, vectorglass, reworkbench\n"
                      "assert hindsight is vectorglass is reworkbench\n"
                      "assert hindsight.api.context['probe'] == 'recreated'\n"
                      "result = hindsight.API_VERSION",
            "context": {"probe": "recreated"},
            "capabilities": [],
        }
        process = subprocess.run([sys.executable, "-I", "-c", runner],
                                 input=json.dumps(request), text=True, capture_output=True, timeout=5)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertIn("__REWORKBENCH_RESULT__2", process.stdout)
        self.assertIn("__REWORKBENCH_ACTIONS__[]", process.stdout)


if __name__ == "__main__":
    unittest.main()
