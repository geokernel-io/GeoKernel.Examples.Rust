# AgentMapAssistant — Rust

Ask for map operations in plain language ("Show the buildings within 150 m of a school in
red") and watch them run as a validated, previewed and recorded GeoKernel workflow. The
planner (an LLM, or the offline demo) only proposes a **workflow DSL v2** plan (ADR-0007);
GeoKernel validates it, the plan is shown before anything runs, steps that change data need
your confirmation of that exact plan, and every executed workflow can be undone and is
recorded with hash-chained provenance. Rust port of the Qt `AgentMapAssistant` example.

The native window's control panel holds the requests and the Run, Confirm and run, Discard,
Undo last and Provenance buttons; the conversation log shows plans and results. The
window's agent session (`window.agent(&runtime)`) keeps its undo journal and provenance
while the window stays usable between calls; the LLM call (`Runtime::propose`) runs on a
worker thread with its own runtime so the window keeps responding.

## Requirements and run

Requires Windows x64, Rust/Cargo with the MSVC toolchain and a valid GeoKernel license. The
agent API ships after geokernel **1.5.40**; until that release, run against the
GeoKernel.Rust sources and a native runtime built from source. This example has its own
Cargo workspace; `run.ps1` patches in the sources for the run:

```powershell
$env:GEOKERNEL_RUST_SOURCE = "D:\projects\GeoKernel.Rust"
$env:GEOKERNEL_BIN = "D:\projects\GeoKernel\outputs\build\Release"
.\run.ps1 -Release
```

Without the patch, geokernel 1.5.40 from crates.io does not compile this example
(`ViewerWindow::agent` is missing). The `Cargo.lock` is not committed while the dependency
is patched; it will be pinned like the other standalone examples after the release.

## Planner: offline demo or an LLM

Without configuration the example runs in **offline demo mode**: the six requests under
**Try** have prepared plans in [data/agent-demo.json](data/agent-demo.json), exactly as a
model would return them, and they go through the same validation, preview, confirmation,
execution, undo and provenance. Other requests are answered with how to enable a model. No
network access is needed.

To plan your own requests (typed under **Your request**, then **Send**), set the provider
before starting:

| Provider | Environment |
|---|---|
| Anthropic | `GEOKERNEL_LLM_PROVIDER=anthropic`, `ANTHROPIC_API_KEY` (model defaults to `claude-opus-5-5`) |
| OpenAI | `GEOKERNEL_LLM_PROVIDER=openai`, `GEOKERNEL_LLM_MODEL`, `OPENAI_API_KEY` |
| Ollama (local) | `GEOKERNEL_LLM_PROVIDER=ollama`, `GEOKERNEL_LLM_MODEL`, optional `GEOKERNEL_LLM_BASE_URL` |

Keys are read by the native code when a request is made and never appear in the log, errors
or provenance. The model receives the request, the layer list with field names and the field
notes from `agent-demo.json` — no feature data.

## Workflow

1. The map shows a synthetic neighbourhood: 352 buildings (`ref`, `use`, `floors`,
   `year_built`, `status`), 4 schools, 3 parks and 16 streets.
2. Pick a request under **Try** and click **Ask**, or type one and click **Send**.
3. The conversation log lists every step of the plan with its category, effects and
   parameters before anything runs. A plan that fails validation is rejected with the reason.
4. **Run** is enabled for read-only and view-only plans. Plans that change or delete data
   enable **Confirm and run** instead, which passes that plan's `planId` as the confirmation.
5. Results follow in the log: tables row by row, measurements with units, feature counts.
6. **Undo last** reverts every step of the most recent workflow.
7. **Provenance** lists the hash-chained execution records.

Edits only apply to result layers in memory; the GeoJSON files are never modified. The data
is a synthetic, deterministic neighbourhood shared with the Qt, .NET, Python and Electron
versions (street names borrowed from Kadıköy, Istanbul; geometry and attributes invented).
Step output hashes are content-based, so the same request gives the same hashes in every
language.
