from pathlib import Path
import unittest
ROOT=Path(__file__).resolve().parents[1]
class Contracts(unittest.TestCase):
 def test_queue_retries(self):
  s=(ROOT/'PlatformServices/CoordinatorService/main.swift').read_text()
  self.assertIn('job.attempt < 20',s)
  self.assertIn('queue.asyncAfter',s)
  self.assertIn('generation == job.generation',s)
 def test_ir_kinds(self):
  s=(ROOT/'App/ContentView.swift').read_text()
  for k in ('"cbranch"','"branch"','"term"','"op"','"data"'):
   self.assertIn(k,s)
if __name__=='__main__': unittest.main()
