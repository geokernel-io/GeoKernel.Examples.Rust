# Shared Rust GeoAI example support

Owned native AI jobs, background input/export/release workers and a profile-based native viewer controller for the new GeoAI examples. GUI objects stay on the OS main thread. `GeoAiExample` exposes ordinary controller operations; `DynamicWorldExample` remains an alias with the Dynamic World default profile. HOTOSM uses `with_profile` and its fixed profile. External validation harnesses live outside the examples repository, with no application test flags.

`runtime` downloads and configures the published 1.5.39 native SDK and synchronizes Windows OS/C runtime environment variables before loading Qt/GDAL. `assets` verifies shared-cache archives with SHA-256 and invokes curl/PowerShell without a shell-built command string. Cancellation stops download/extraction child processes and removes only the worker's owned temporary files. ZIP preparation handles Rust's canonical Windows path prefix explicitly for .NET Framework compatibility.

All seven GeoAI examples are standalone workspaces with their own lockfiles. This common crate uses geokernel 1.5.39 from crates.io and the matching SHA-256 verified GitHub release runtime. Existing unrelated example pins remain unchanged.
