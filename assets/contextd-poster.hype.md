---
title: "contextd Benchmark Poster"
theme: matte-black
font: "Noto Sans"
color_background: "#13171A"
color_foreground: "#EFF2F5"
color_dark_foreground: "#A6B2BC"
color_accent: "#4EC2A8"
color_yellow: "#E5A93C"
---

```svg
<svg viewBox="0 0 1600 900" xmlns="http://www.w3.org/2000/svg">
  <!-- Background plate -->
  <rect x="0" y="0" width="1600" height="900" fill="var(--background)"/>

  <!-- Instrument framing rules -->
  <line x1="88" y1="72" x2="1512" y2="72" stroke="#303A42" stroke-width="2"/>
  <line x1="88" y1="828" x2="1512" y2="828" stroke="#303A42" stroke-width="1.5"/>
  <line x1="636" y1="108" x2="636" y2="796" stroke="#2A333A" stroke-width="1.5"/>

  <!-- Left column: identity and measured headline stats -->
  <text x="88" y="132" font-family="JetBrainsMono Nerd Font" font-size="18" fill="var(--dark-foreground)">Local-first MCP and hybrid search</text>

  <text x="88" y="236" font-family="Noto Sans" font-size="84" font-weight="700" fill="var(--foreground)">contextd</text>

  <text x="88" y="294" font-family="Noto Sans" font-size="24" fill="var(--dark-foreground)">Tree-sitter AST chunking, local ONNX</text>
  <text x="88" y="328" font-family="Noto Sans" font-size="24" fill="var(--dark-foreground)">embeddings, and SQLite WAL on loopback.</text>

  <rect x="88" y="376" width="76" height="4" fill="var(--accent)"/>

  <!-- Metric 1 -->
  <text x="88" y="482" font-family="JetBrainsMono Nerd Font" font-size="68" font-weight="700" fill="var(--accent)">6.44 ms</text>
  <text x="88" y="518" font-family="Noto Sans" font-size="20" fill="var(--dark-foreground)">Warm p50 hybrid query (p95: 7.74 ms)</text>

  <line x1="88" y1="562" x2="580" y2="562" stroke="#2A333A" stroke-width="1.5"/>

  <!-- Metric 2 -->
  <text x="88" y="660" font-family="JetBrainsMono Nerd Font" font-size="68" font-weight="700" fill="var(--foreground)">-47.6%</text>
  <text x="88" y="696" font-family="Noto Sans" font-size="20" fill="var(--dark-foreground)">Peak RSS after 16-chunk micro-batching</text>

  <!-- Colophon -->
  <text x="88" y="788" font-family="JetBrainsMono Nerd Font" font-size="16" fill="var(--dark-foreground)">v4.0.2  |  Zero cloud APIs  |  MIT</text>

  <!-- Right column: Panel A - ONNX Micro-Batch Fix -->
  <text x="692" y="132" font-family="Noto Sans" font-size="23" font-weight="700" fill="var(--foreground)">Capping the ONNX CPU memory arena</text>
  <text x="692" y="162" font-family="JetBrainsMono Nerd Font" font-size="15" fill="var(--dark-foreground)">Indexing contextd (79 files, 1,262 chunks) with all-minilm-l6-v2</text>

  <!-- Before card -->
  <rect x="692" y="192" width="820" height="146" fill="#1D2429" stroke="#36414A" stroke-width="1.5"/>
  <text x="720" y="226" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="var(--yellow)">Before: Unbounded per-file batch (100+ chunks)</text>
  <rect x="720" y="246" width="520" height="42" fill="var(--yellow)"/>
  <text x="738" y="273" font-family="JetBrainsMono Nerd Font" font-size="15" font-weight="700" fill="#13171A">1x [N, max_seq] tensor holds Mutex&lt;Session&gt;</text>
  <text x="1484" y="266" text-anchor="end" font-family="JetBrainsMono Nerd Font" font-size="24" font-weight="700" fill="var(--yellow)">2,246.1 MB</text>
  <text x="1484" y="294" text-anchor="end" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">16.5 s first-query stall</text>
  <text x="720" y="320" font-family="Noto Sans" font-size="15" fill="var(--dark-foreground)">BatchLongest padding bloated attention memory and starved /query.</text>

  <!-- After card -->
  <rect x="692" y="362" width="820" height="146" fill="#1D2429" stroke="var(--accent)" stroke-width="1.5"/>
  <text x="720" y="396" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="var(--accent)">After: MAX_EMBED_BATCH_SIZE = 16 micro-batches</text>

  <!-- 6 clean micro-batch slices with subtle mutex-yield ticks between them -->
  <rect x="720" y="416" width="74" height="42" fill="var(--accent)"/>
  <text x="757" y="443" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="#13171A">16</text>
  <line x1="802" y1="418" x2="802" y2="456" stroke="#465460" stroke-width="1.5" stroke-dasharray="3,3"/>

  <rect x="810" y="416" width="74" height="42" fill="var(--accent)"/>
  <text x="847" y="443" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="#13171A">16</text>
  <line x1="892" y1="418" x2="892" y2="456" stroke="#465460" stroke-width="1.5" stroke-dasharray="3,3"/>

  <rect x="900" y="416" width="74" height="42" fill="var(--accent)"/>
  <text x="937" y="443" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="#13171A">16</text>
  <line x1="982" y1="418" x2="982" y2="456" stroke="#465460" stroke-width="1.5" stroke-dasharray="3,3"/>

  <rect x="990" y="416" width="74" height="42" fill="var(--accent)"/>
  <text x="1027" y="443" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="#13171A">16</text>
  <line x1="1072" y1="418" x2="1072" y2="456" stroke="#465460" stroke-width="1.5" stroke-dasharray="3,3"/>

  <rect x="1080" y="416" width="74" height="42" fill="var(--accent)"/>
  <text x="1117" y="443" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="#13171A">16</text>
  <line x1="1162" y1="418" x2="1162" y2="456" stroke="#465460" stroke-width="1.5" stroke-dasharray="3,3"/>

  <rect x="1170" y="416" width="74" height="42" fill="var(--accent)"/>
  <text x="1207" y="443" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="16" font-weight="700" fill="#13171A">16</text>

  <text x="1484" y="436" text-anchor="end" font-family="JetBrainsMono Nerd Font" font-size="24" font-weight="700" fill="var(--accent)">1,177.6 MB</text>
  <text x="1484" y="464" text-anchor="end" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">5.0 MB SQLite WAL DB</text>
  <text x="720" y="490" font-family="Noto Sans" font-size="15" fill="var(--dark-foreground)">Mutex releases between slices; concurrent hybrid queries stay fast.</text>

  <!-- Panel separator -->
  <line x1="692" y1="544" x2="1512" y2="544" stroke="#2A333A" stroke-width="1.5"/>

  <!-- Right column: Panel B - Warm Hybrid Query Latency Scale -->
  <text x="692" y="586" font-family="Noto Sans" font-size="23" font-weight="700" fill="var(--foreground)">Steady-state hybrid query latency (POST /query)</text>
  <text x="692" y="614" font-family="JetBrainsMono Nerd Font" font-size="15" fill="var(--dark-foreground)">Query ONNX embedding + sqlite-vec KNN + FTS5 RRF across 15 queries</text>

  <!-- Linear scale: 0 ms at x=692, 10 ms at x=1512 (82 px/ms) -->
  <line x1="692" y1="706" x2="1512" y2="706" stroke="#465460" stroke-width="2"/>

  <!-- Ticks: 0, 2, 4, 6, 8, 10 ms (with generous breathing room below range band) -->
  <line x1="692" y1="696" x2="692" y2="716" stroke="#465460" stroke-width="2"/>
  <text x="692" y="750" text-anchor="start" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">0 ms</text>

  <line x1="856" y1="698" x2="856" y2="714" stroke="#465460" stroke-width="1.5"/>
  <text x="856" y="750" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">2 ms</text>

  <line x1="1020" y1="698" x2="1020" y2="714" stroke="#465460" stroke-width="1.5"/>
  <text x="1020" y="750" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">4 ms</text>

  <line x1="1184" y1="698" x2="1184" y2="714" stroke="#465460" stroke-width="1.5"/>
  <text x="1184" y="750" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">6 ms</text>

  <line x1="1348" y1="698" x2="1348" y2="714" stroke="#465460" stroke-width="1.5"/>
  <text x="1348" y="750" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">8 ms</text>

  <line x1="1512" y1="696" x2="1512" y2="716" stroke="#465460" stroke-width="2"/>
  <text x="1512" y="750" text-anchor="end" font-family="JetBrainsMono Nerd Font" font-size="14" fill="var(--dark-foreground)">10 ms</text>

  <!-- Observed range band: min 5.64ms (x=1154) to p95 7.74ms (x=1327) -->
  <rect x="1154" y="690" width="173" height="32" fill="#204540" stroke="var(--accent)" stroke-width="2"/>

  <!-- p50 marker: 6.44ms (x=1220) -->
  <line x1="1220" y1="666" x2="1220" y2="722" stroke="var(--accent)" stroke-width="3.5"/>
  <circle cx="1220" cy="706" r="6.5" fill="var(--accent)"/>

  <!-- Non-overlapping callout labels -->
  <text x="1142" y="680" text-anchor="end" font-family="JetBrainsMono Nerd Font" font-size="15" fill="var(--dark-foreground)">min 5.64 ms</text>
  <text x="1220" y="654" text-anchor="middle" font-family="JetBrainsMono Nerd Font" font-size="18" font-weight="700" fill="var(--accent)">p50 6.44 ms</text>
  <text x="1340" y="680" text-anchor="start" font-family="JetBrainsMono Nerd Font" font-size="15" fill="var(--foreground)">p95 7.74 ms</text>

  <!-- Cold CLI footer -->
  <text x="692" y="792" font-family="JetBrainsMono Nerd Font" font-size="15" fill="var(--dark-foreground)">Cold CLI query (process start + ONNX load): 190.8 ms median</text>
</svg>
```
