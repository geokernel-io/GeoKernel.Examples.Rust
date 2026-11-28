# Spatial embedding search — Rust

Search the AlphaEarth embeddings of the Netherlands for places that look like a seed place, limited to
one CRS, one extent and a range of years, with the number of records each filter rejected. Rust port of
the Qt `SpatialEmbeddingSearch` example, built on `Runtime::embedding_index` (geokernel `EmbeddingIndex`).
The control panel picks the query: the "Search in" list sets both the CRS and the extent. The docked
**Filters** panel shows what each filter rejected.

## Run

The embedding index ships after geokernel **1.5.40**. Until then, patch in the GeoKernel.Rust sources:

```powershell
$env:GEOKERNEL_RUST_SOURCE = "D:\projects\GeoKernel.Rust"
$env:GEOKERNEL_BIN = "D:\projects\GeoKernel\outputs\build\Release"
.\run.ps1 -Release            # .\run.ps1 -Release --smoke runs the three checks and exits
```

## What the filters do

Every record keeps its own extent, CRS and year, and a query states all three. The index walks its
HNSW graph by similarity and keeps only the records that pass the filters. It counts the ones each
filter rejected, and a result never contains a record that fails a filter.

- **CRS.** A query in EPSG:32631 only sees records in EPSG:32631, so extents are always compared in
  the same coordinate system.
- **Cross-zone search.** A seed cell from one zone can be searched in the other: its embedding is
  compared, its extent never is.
- **Extent.** Records outside the query extent (in the query's CRS) are rejected.
- **Time.** Records whose year lies outside the query years are rejected.

The `--smoke` run makes three searches that each show one filter. It checks that every match passes
all three filters, and gives the same numbers on every platform:

| Search | Matches (best similarity) | Rejected by CRS / extent / time |
|---|---|---|
| Solar park 2024, zone 31, 2018 only | 20 (0.925) | 495 / 0 / 1 006 |
| Greenhouses 2024, zone 31, Randstad, all years | 20 (1.000) | 52 / 175 / 0 |
| Solar park 2024 (zone 31), searched in zone 32, Groningen and Drenthe, 2024 | 20 (0.961) | 3 773 / 392 / 721 |

Building the index takes about 10–30 s depending on the platform; each query takes 1–5 ms.

## Workflow

1. **Look for places like:** pick a seed place, for example the Zuyderzon solar park in Almere,
   Westland greenhouses, the Veluwe forest, Amsterdam centre, the Markermeer or Groningen farmland.
2. **Seed year:** the year of the seed cell's embedding.
3. **Search in:** the CRS and the extent: one zone as a whole, or a region such as Randstad,
   Flevoland, Groningen and Drenthe, or Twente and Achterhoek.
4. **Years:** the time filter, a single year or a range.
5. **Search.** The panel shows the matches and how many records the CRS, extent and time filters
   rejected. On the map:
   - the matches are red;
   - the seed is blue;
   - the query extent is yellow, drawn in its UTM zone and projected onto the RD New map;
   - the preview of the last year of the range is the background.

## Credits

The AlphaEarth Foundations Satellite Embedding dataset is produced by Google and Google DeepMind
(CC BY 4.0). The 2018, 2021 and 2024 layers come from the Source Cooperative mirror, resampled to
1 km by GeoKernel.

## Data

`netherlands_alphaearth_2018_2024_1km_v1.zip` (GeoKernel.SampleData v1, 14 MB, SHA-256 checked and cached
on the first run):

- **Embeddings.** AlphaEarth Foundations annual satellite embeddings for 2018, 2021 and 2024. One 1 km
  grid per year and UTM zone: EPSG:32631 west of 6° E (193 × 319 cells) and EPSG:32632 east of it
  (97 × 317 cells). Each cell has 64 values, stored as signed bytes (×127). There are 227 424 cells
  with data.
- **`embeddings.json`.** The grid layout, six seed places and six query extents. Each extent is given
  in its own CRS, with a WGS84 outline for drawing.
- **`centers.bin`.** The WGS84 centre of every cell, used to draw the matches.
- **`preview_<year>.tif`.** A false-colour picture of each year in RD New (EPSG:28992): the first
  three principal components of the 2024 vectors. Water is green, towns red and forests purple.

The data was built with GeoKernel.Cloud `EmbeddingCollection` from the 640 m overview level of the
Source Cooperative mirror (59 MB read).
