# LayerStylingAndFiltering — Rust

Automatically prepares the published SDK and Sagrada Família DEM, orthophoto,
roads and buildings using the existing Rust sample cache.

Supports per-layer visibility, opacity, uniform/source colors, attribute themes,
literal case-insensitive substring filters, example values, matching counts and
zoom to matches. Press Enter or leave the Contains field to apply text.
Example values are limited to 200 per field; filtering searches every record.
Counts include records without drawable geometry. Changes are session-only.

Uses the published GeoKernel 1.5.32 package. No local SDK build or DLL override is required.

CMD:

```cmd
cd /d "D:\projects\GeoKernel.Examples.Rust" && powershell -NoProfile -ExecutionPolicy Bypass -File "LayerStylingAndFiltering\run.ps1" -Release
```
