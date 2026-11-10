param(
  [string]$Url = 'https://127.0.0.1:8443/v1',
  [string]$ServerPath = ''
)
$ErrorActionPreference = 'Stop'
if (-not (Get-Command codex -ErrorAction SilentlyContinue)) { throw 'Install Codex CLI first.' }
if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Install Node.js first.' }
if (-not $ServerPath) { $ServerPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'mcp/hindsight-mcp.mjs' }
if (-not (Test-Path -LiteralPath $ServerPath -PathType Leaf)) { throw "Missing HindSight MCP server: $ServerPath" }
$secure = Read-Host 'HindSight bearer token' -AsSecureString
$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try { $token = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr) }
finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
if (-not $token) { throw 'A non-empty HindSight token is required.' }
& codex mcp remove hindsight 2>$null | Out-Null
& codex mcp add hindsight --env "HINDSIGHT_URL=$Url" --env "HINDSIGHT_TOKEN=$token" -- node $ServerPath
if ($LASTEXITCODE -ne 0) { throw 'Codex MCP registration failed.' }
$token = $null
Write-Host 'HindSight MCP registered with Codex. Restart Codex to load the tools.'
