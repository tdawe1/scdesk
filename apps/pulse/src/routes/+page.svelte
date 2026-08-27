<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  type Quote = {
    id: string;
    last: number;
    change: number;
    change_pct: number;
  };
  type Metric = { name: string; value: string; note: string };
  type Pillar = {
    id: string;
    name: string;
    score: number;
    weight: number;
    metrics: Metric[];
  };
  type CalEvent = {
    title: string;
    country: string;
    ts: number;
    impact: string;
    forecast: string;
    previous: string;
    actual: string;
    is_macro: boolean;
  };
  type Dashboard = {
    mode: string;
    quotes: Quote[];
    fetched_at_unix: number;
    stale: boolean;
    errors: string[];
    score: {
      composite: number;
      decision: string;
      size: string;
      bias: {
        label: string;
        score: number;
        daily: string;
        weekly: string;
        monthly: string;
      };
      pillars: Pillar[];
    };
    calendar: CalEvent[];
    score_history: { ts: number; composite: number }[];
    has_fmp_key: boolean;
    poll_secs: number;
    theme: string;
    zoom: number;
    pre_event_alert_min: number;
    alert_on_release: boolean;
    alert_on_decision: boolean;
    correlations: { symbol: string; corr: number }[];
    earnings: { symbol: string; ts: number }[];
    banners: { level: string; text: string }[];
    fired_alerts: { kind: string; text: string }[];
  };

  const CORE = ["SPY", "QQQ", "VIX", "TNX", "DXY"];
  const POLL_MS = 30_000;

  let dash = $state<Dashboard | null>(null);
  let error = $state<string | null>(null);
  let pinned = $state(false);
  let loading = $state(false);
  let now = $state(Math.floor(Date.now() / 1000));
  let settingsOpen = $state(false);
  let fmpDraft = $state("");
  let showHigh = $state(true);
  let showMed = $state(true);
  let showLow = $state(true);
  let showDone = $state(false);
  let countryOn = $state<Record<string, boolean>>({});
  let lastAlertKey = $state("");

  const countries = $derived(
    [...new Set((dash?.calendar ?? []).map((e) => e.country))].sort(),
  );
  const filteredCal = $derived(
    (dash?.calendar ?? []).filter((e) => {
      if (e.impact === "High" && !showHigh) return false;
      if (e.impact === "Medium" && !showMed) return false;
      if (e.impact === "Low" && !showLow) return false;
      if (!showDone && e.ts < now) return false;
      const active = Object.entries(countryOn)
        .filter(([, v]) => v)
        .map(([k]) => k);
      if (active.length && !active.includes(e.country)) return false;
      return true;
    }),
  );

  const coreQuotes = $derived(
    (dash?.quotes ?? []).filter((q) => CORE.includes(q.id)),
  );
  const sectorQuotes = $derived(
    (dash?.quotes ?? [])
      .filter((q) => !CORE.includes(q.id))
      .slice()
      .sort((a, b) => b.change_pct - a.change_pct),
  );
  const sectorMax = $derived(
    Math.max(0.8, ...sectorQuotes.map((q) => Math.abs(q.change_pct))),
  );

  function fmt(n: number, d = 2): string {
    return n.toLocaleString("en-US", {
      minimumFractionDigits: d,
      maximumFractionDigits: d,
    });
  }
  function fmtChg(n: number): string {
    return `${n > 0 ? "+" : ""}${fmt(n)}`;
  }
  function fmtPct(n: number): string {
    return `${n > 0 ? "+" : ""}${fmt(n)}%`;
  }
  function cls(n: number): string {
    if (n > 0) return "up";
    if (n < 0) return "down";
    return "flat";
  }
  function arrow(a: string): string {
    if (a === "Up") return "▲";
    if (a === "Down") return "▼";
    return "■";
  }
  function countdown(ts: number): string {
    const s = ts - now;
    if (s <= 0) return "now";
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = s % 60;
    if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
    return `${m}m ${String(sec).padStart(2, "0")}s`;
  }
  function spark(hist: { composite: number }[]): string {
    if (hist.length < 2) return "";
    const w = 220;
    const h = 48;
    const min = Math.min(...hist.map((p) => p.composite), 40);
    const max = Math.max(...hist.map((p) => p.composite), 80);
    const span = Math.max(1, max - min);
    return hist
      .map((p, i) => {
        const x = (i / (hist.length - 1)) * w;
        const y = h - ((p.composite - min) / span) * h;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  }
  function sectorWidth(pct: number): string {
    return `${(Math.abs(pct) / sectorMax) * 50}%`;
  }

  async function refresh(force = false) {
    if (loading && !force) return;
    loading = true;
    try {
      const next = await invoke<Dashboard>("get_dashboard", { force });
      dash = next;
      error = next.errors.length ? next.errors.slice(0, 3).join(" · ") : null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
      now = Math.floor(Date.now() / 1000);
    }
  }

  async function setMode(mode: string) {
    loading = true;
    try {
      dash = await invoke<Dashboard>("set_mode", { mode });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function togglePin() {
    pinned = !pinned;
    try {
      await getCurrentWindow().setAlwaysOnTop(pinned);
    } catch {
      pinned = !pinned;
    }
  }

  async function saveKey() {
    await invoke("set_fmp_key", { key: fmpDraft });
    fmpDraft = "";
    settingsOpen = false;
    void refresh(true);
  }

  function onKey(ev: KeyboardEvent) {
    if (settingsOpen) return;
    if (ev.key === "t" || ev.key === "T") {
      if (ev.ctrlKey || ev.metaKey) return;
      ev.preventDefault();
      void togglePin();
    }
    if (ev.key === "d" || ev.key === "D") {
      if (ev.ctrlKey || ev.metaKey) return;
      ev.preventDefault();
      void setMode("day");
    }
    if (ev.key === "s" || ev.key === "S") {
      if (ev.ctrlKey || ev.metaKey) return;
      ev.preventDefault();
      void setMode("swing");
    }
    if ((ev.key === "r" || ev.key === "R") && (ev.ctrlKey || ev.metaKey)) {
      ev.preventDefault();
      void refresh(true);
    }
    if (ev.key === "Delete") {
      ev.preventDefault();
      void getCurrentWindow().minimize();
    }
  }

  function beep() {
    const ctx = new AudioContext();
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.connect(g);
    g.connect(ctx.destination);
    o.frequency.value = 880;
    g.gain.value = 0.06;
    o.start();
    o.stop(ctx.currentTime + 0.12);
  }

  function toast(text: string) {
    beep();
    if (typeof Notification !== "undefined") {
      if (Notification.permission === "granted") {
        new Notification("scdesk Pulse", { body: text });
      } else if (Notification.permission !== "denied") {
        void Notification.requestPermission();
      }
    }
  }

  async function persist(partial: Record<string, unknown>) {
    if (!dash) return;
    const settings = {
      mode: dash.mode,
      fmp_api_key: fmpDraft,
      poll_secs: dash.poll_secs,
      theme: dash.theme,
      zoom: dash.zoom,
      pre_event_alert_min: dash.pre_event_alert_min,
      alert_on_release: dash.alert_on_release,
      alert_on_decision: dash.alert_on_decision,
      ...partial,
    };
    try {
      dash = await invoke<Dashboard>("save_settings", { settings });
    } catch (e) {
      error = String(e);
    }
  }

  function fitZoom() {
    const z = Math.max(
      100,
      Math.min(180, Math.floor(Math.min(window.innerWidth / 1280, window.innerHeight / 860) * 100)),
    );
    void persist({ zoom: z });
  }

  $effect(() => {
    const theme = dash?.theme ?? "dark";
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.setProperty("--zoom", String((dash?.zoom ?? 100) / 100));
  });

  $effect(() => {
    const alerts = dash?.fired_alerts ?? [];
    if (!alerts.length) return;
    const key = alerts.map((a) => a.text).join("|");
    if (key === lastAlertKey) return;
    lastAlertKey = key;
    for (const a of alerts) toast(a.text);
  });

  onMount(() => {
    void refresh(true);
    const clock = setInterval(() => {
      now = Math.floor(Date.now() / 1000);
    }, 1000);
    window.addEventListener("keydown", onKey);
    return () => {
      clearInterval(clock);
      window.removeEventListener("keydown", onKey);
    };
  });

  $effect(() => {
    const ms = Math.max(15, dash?.poll_secs ?? 30) * 1000;
    const poll = setInterval(() => void refresh(false), ms);
    return () => clearInterval(poll);
  });
</script>

<div class="desk">
  <header class="topbar">
    <div class="brand">scdesk pulse</div>
    <div class="modes">
      <button class:on={dash?.mode === "day"} onclick={() => setMode("day")}>DAY</button>
      <button class:on={dash?.mode === "swing"} onclick={() => setMode("swing")}>SWING</button>
    </div>
    <div class="status" class:stale={dash?.stale !== false} class:live={dash && !dash.stale}>
      <span class="dot"></span>
      {dash && !dash.stale ? "LIVE" : "STALE"}
    </div>
    <div class="clock">
      {dash ? new Date(dash.fetched_at_unix * 1000).toLocaleTimeString() : "—"}
      {#if loading}<span> · loading</span>{/if}
    </div>
    <div class="spacer"></div>
    {#if error}
      <div class="err" title={error}>{error}</div>
    {/if}
    <label class="clock">poll
      <select
        value={String(dash?.poll_secs ?? 30)}
        onchange={(e) => persist({ poll_secs: Number(e.currentTarget.value) })}
      >
        <option value="15">15s</option>
        <option value="30">30s</option>
        <option value="45">45s</option>
        <option value="120">2m</option>
      </select>
    </label>
    <label class="clock">zoom
      <select
        value={String(dash?.zoom ?? 100)}
        onchange={(e) => persist({ zoom: Number(e.currentTarget.value) })}
      >
        {#each [100, 110, 125, 150, 180] as z}
          <option value={z}>{z}%</option>
        {/each}
      </select>
    </label>
    <button type="button" onclick={fitZoom}>FIT</button>
    <button
      type="button"
      onclick={() => persist({ theme: dash?.theme === "light" ? "dark" : "light" })}
    >{dash?.theme === "light" ? "dark" : "light"}</button>
    <button type="button" onclick={() => refresh(true)}>refresh</button>
    <button type="button" class:on={pinned} onclick={togglePin}>{pinned ? "pinned" : "pin"}</button>
    <button type="button" onclick={() => (settingsOpen = true)}>settings</button>
  </header>

  {#if dash?.banners?.length}
    <div class="banners">
      {#each dash.banners as b}
        <div class="banner {b.level}">{b.text}</div>
      {/each}
    </div>
  {/if}

  <section class="block">
    <h2>Indexes</h2>
    <div class="indexes">
      {#each coreQuotes as q (q.id)}
        <article class={cls(q.change)}>
          <div class="sym">{q.id}</div>
          <div class="last">{fmt(q.last)}</div>
          <div class="chg">{fmtChg(q.change)} · {fmtPct(q.change_pct)}</div>
        </article>
      {:else}
        <div class="empty">waiting for quotes…</div>
      {/each}
    </div>
  </section>

  {#if dash}
    {@const s = dash.score}
    <section class="block">
      <h2>Session call</h2>
      <div class="hero">
        <article class="hero-card">
          <div class="k">trade today?</div>
          <div class="decision {s.decision.toLowerCase()}">{s.decision}</div>
        </article>
        <article class="hero-card gauge-card">
          <div
            class="gauge"
            style="background: conic-gradient(var(--live) {s.composite}%, #1e2a3a 0)"
          >
            <div class="gauge-inner">
              <strong>{fmt(s.composite, 1)}</strong>
              <span>quality</span>
            </div>
          </div>
        </article>
        <article class="hero-card">
          <div class="k">direction</div>
          <div class="bias-lab {s.bias.label.toLowerCase()}">{s.bias.label}</div>
          <div class="tf">
            <span class={s.bias.daily.toLowerCase()}>D {arrow(s.bias.daily)}</span>
            <span class={s.bias.weekly.toLowerCase()}>W {arrow(s.bias.weekly)}</span>
            <span class={s.bias.monthly.toLowerCase()}>M {arrow(s.bias.monthly)}</span>
          </div>
          <div class="k">bias {fmt(s.bias.score, 0)}</div>
        </article>
        <article class="hero-card">
          <div class="k">size</div>
          <div class="size-val">{s.size}</div>
        </article>
        <article class="hero-card spark-card">
          <div class="k">6h quality</div>
          {#if dash.score_history.length > 1}
            <svg viewBox="0 0 220 48" preserveAspectRatio="none">
              <polyline
                fill="none"
                stroke="#3ddc97"
                stroke-width="1.8"
                points={spark(dash.score_history)}
              />
            </svg>
          {:else}
            <div class="k">building…</div>
          {/if}
        </article>
      </div>
    </section>

    <section class="block">
      <h2>Weights</h2>
      <div class="weights">
        {#each s.pillars as p}
          <div class="wcol">
            <i style="height: {Math.round(p.weight * 100)}%"></i>
            <span>{p.name.slice(0, 3)}</span>
            <b>{Math.round(p.weight * 100)}%</b>
          </div>
        {/each}
      </div>
    </section>

    <section class="block">
      <h2>Pillars <span>{dash.mode.toUpperCase()}</span></h2>
      <div class="pillars">
        {#each s.pillars as p (p.id)}
          <article>
            <div class="phead">
              <span>{p.name}</span>
              <strong>{fmt(p.score, 1)}</strong>
            </div>
            <div class="bar"><i style="width: {p.score}%"></i></div>
            <div class="k">{Math.round(p.weight * 100)}% of composite</div>
            <dl>
              {#each p.metrics as m}
                <dt title={m.note}>{m.name}</dt>
                <dd title={m.note}>{m.value}</dd>
              {/each}
            </dl>
          </article>
        {/each}
      </div>
    </section>
  {/if}

  {#if dash?.correlations?.length}
    <section class="block">
      <h2>SPY 20d correlation</h2>
      <div class="heat">
        {#each dash.correlations as c}
          <div
            class="hcell"
            style="background: color-mix(in srgb, {c.corr >= 0 ? 'var(--up)' : 'var(--down)'} {Math.round(
              Math.abs(c.corr) * 85,
            )}%, var(--panel))"
            title="{c.symbol} {c.corr.toFixed(2)}"
          >
            <span>{c.symbol}</span>
            <b>{c.corr.toFixed(2)}</b>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if sectorQuotes.length}
    <section class="block">
      <h2>Sectors <span>day change</span></h2>
      <div class="sectors">
        {#each sectorQuotes as q (q.id)}
          <div class="srow {cls(q.change)}">
            <div class="sid">{q.id}</div>
            <div class="sbar">
              <span class="mid"></span>
              {#if q.change_pct >= 0}
                <i class="pos" style="width: {sectorWidth(q.change_pct)}; left: 50%"></i>
              {:else}
                <i class="neg" style="width: {sectorWidth(q.change_pct)}; right: 50%"></i>
              {/if}
            </div>
            <div class="spct">{fmtPct(q.change_pct)}</div>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <section class="block cal-block">
    <h2>Calendar <span>this week</span></h2>
    <div class="chips">
      <button class:on={showHigh} onclick={() => (showHigh = !showHigh)}>High</button>
      <button class:on={showMed} onclick={() => (showMed = !showMed)}>Medium</button>
      <button class:on={showLow} onclick={() => (showLow = !showLow)}>Low</button>
      <button class:on={showDone} onclick={() => (showDone = !showDone)}>Done</button>
      {#each countries as c}
        <button
          class:on={countryOn[c] !== false}
          onclick={() => (countryOn = { ...countryOn, [c]: countryOn[c] === false })}
        >{c}</button>
      {/each}
    </div>
    <div class="cal">
      {#if filteredCal.length}
        {#each filteredCal as e, i (e.ts + e.title + i)}
          <div
            class="ev"
            class:high={e.impact === "High"}
            class:macro={e.is_macro}
            class:soon={e.ts - now < 3600 && e.ts >= now}
            class:imminent={e.ts - now < 300 && e.ts >= now}
          >
            <div class="when">{countdown(e.ts)}</div>
            <div class="ttl">{e.country} · {e.title}</div>
            <div class="nums">
              {e.impact}
              {#if e.forecast} · F {e.forecast}{/if}
              {#if e.previous} · P {e.previous}{/if}
              {#if e.actual} · A {e.actual}{/if}
            </div>
          </div>
        {/each}
      {:else}
        <div class="empty">no calendar yet</div>
      {/if}
    </div>
  </section>
</div>

{#if settingsOpen}
  <div class="modal" role="dialog">
    <div class="panel">
      <h2>settings</h2>
      <p class="k">
        FMP key fills Actuals and is the calendar fallback if Forex Factory is down.
      </p>
      <input
        type="password"
        placeholder={dash?.has_fmp_key ? "key saved — paste to replace" : "FMP API key"}
        bind:value={fmpDraft}
      />
      <label class="k">pre-event alert
        <select
          value={String(dash?.pre_event_alert_min ?? 15)}
          onchange={(e) => persist({ pre_event_alert_min: Number(e.currentTarget.value) })}
        >
          <option value="0">off</option>
          <option value="5">5m</option>
          <option value="15">15m</option>
          <option value="30">30m</option>
          <option value="60">60m</option>
        </select>
      </label>
      <label class="k"
        ><input
          type="checkbox"
          checked={dash?.alert_on_decision ?? true}
          onchange={(e) => persist({ alert_on_decision: e.currentTarget.checked })}
        /> alert on decision / bias</label
      >
      <label class="k"
        ><input
          type="checkbox"
          checked={dash?.alert_on_release ?? false}
          onchange={(e) => persist({ alert_on_release: e.currentTarget.checked })}
        /> alert on actuals</label
      >
      <div class="row">
        <button type="button" onclick={saveKey}>save key</button>
        <button type="button" onclick={() => (settingsOpen = false)}>close</button>
      </div>
      <p class="k">keys: D day · S swing · T pin · Ctrl+R refresh · Del minimize</p>
    </div>
  </div>
{/if}

<style>
  .desk {
    height: 100%;
    overflow: auto;
    padding: 16px 18px 24px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .topbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    position: sticky;
    top: 0;
    z-index: 2;
    background: color-mix(in srgb, var(--bg) 92%, transparent);
    padding: 4px 0 10px;
    border-bottom: 1px solid var(--border);
  }
  .brand {
    letter-spacing: 0.14em;
    text-transform: uppercase;
    font-size: 11px;
    color: var(--muted);
  }
  .modes {
    display: flex;
    gap: 4px;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 700;
  }
  .status .dot {
    width: 7px;
    height: 7px;
    border-radius: 99px;
    background: currentColor;
  }
  .status.live {
    color: var(--live);
  }
  .status.stale {
    color: var(--stale);
  }
  .clock,
  .k,
  .err {
    color: var(--muted);
    font-size: 11px;
  }
  .err {
    max-width: min(420px, 40vw);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--stale);
  }
  .spacer {
    flex: 1;
    min-width: 12px;
  }
  button {
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 5px 10px;
    font: inherit;
    cursor: pointer;
  }
  button.on {
    border-color: var(--live);
    color: var(--live);
  }

  .block h2 {
    margin: 0 0 10px;
    font-size: 11px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  .block h2 span {
    font-weight: 400;
    margin-left: 8px;
    opacity: 0.7;
  }

  .indexes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 10px;
  }
  .indexes article {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px 14px;
  }
  .sym {
    font-size: 11px;
    color: var(--muted);
    letter-spacing: 0.08em;
  }
  .last {
    font-size: 22px;
    font-weight: 700;
    margin: 4px 0 2px;
  }
  .chg {
    font-size: 12px;
  }
  .up .last,
  .up .chg,
  .up .spct {
    color: var(--up);
  }
  .down .last,
  .down .chg,
  .down .spct {
    color: var(--down);
  }
  .empty {
    color: var(--muted);
    grid-column: 1 / -1;
    padding: 12px;
  }

  .hero {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 10px;
  }
  .hero-card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px 16px;
    min-height: 110px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 6px;
  }
  .gauge-card,
  .spark-card {
    align-items: center;
  }
  .decision {
    font-size: clamp(22px, 3vw, 32px);
    font-weight: 800;
    letter-spacing: 0.04em;
  }
  .decision.yes {
    color: var(--up);
  }
  .decision.caution {
    color: var(--stale);
  }
  .decision.no {
    color: var(--down);
  }
  .gauge {
    width: 96px;
    height: 96px;
    border-radius: 50%;
    display: grid;
    place-items: center;
  }
  .gauge-inner {
    width: 70px;
    height: 70px;
    border-radius: 50%;
    background: var(--bg);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }
  .gauge-inner strong {
    font-size: 20px;
  }
  .bias-lab {
    font-size: clamp(22px, 3vw, 30px);
    font-weight: 800;
  }
  .bias-lab.long {
    color: var(--up);
  }
  .bias-lab.short {
    color: var(--down);
  }
  .tf {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
  }
  .tf .up {
    color: var(--up);
  }
  .tf .down {
    color: var(--down);
  }
  .size-val {
    font-size: clamp(22px, 3vw, 30px);
    font-weight: 800;
  }
  .spark-card svg {
    width: 100%;
    height: 48px;
  }

  .pillars {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 10px;
  }
  .pillars article {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px 14px;
  }
  .phead {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
    margin-bottom: 6px;
  }
  .phead strong {
    font-size: 18px;
  }
  .bar {
    height: 4px;
    background: #1e2a3a;
    border-radius: 2px;
    margin-bottom: 6px;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--live);
    border-radius: 2px;
  }
  .pillars dl {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px 10px;
    margin: 10px 0 0;
    font-size: 12px;
  }
  dt {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .sectors {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 12px;
  }
  .srow {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr) 72px;
    gap: 10px;
    align-items: center;
  }
  .sid {
    color: var(--muted);
    font-size: 12px;
  }
  .sbar {
    position: relative;
    height: 10px;
    background: var(--panel-2);
    border-radius: 4px;
    overflow: hidden;
  }
  .sbar .mid {
    position: absolute;
    left: 50%;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--border);
  }
  .sbar i {
    position: absolute;
    top: 0;
    bottom: 0;
  }
  .sbar i.pos {
    background: var(--up);
  }
  .sbar i.neg {
    background: var(--down);
  }
  .spct {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .cal {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 10px;
  }
  .ev {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 12px;
    min-height: 88px;
  }
  .ev.high {
    border-color: #8a4a12;
  }
  .ev.macro {
    border-color: var(--stale);
  }
  .ev.imminent {
    animation: blink 1s step-end infinite;
  }
  @keyframes blink {
    50% {
      border-color: var(--down);
    }
  }
  .when {
    color: var(--stale);
    font-weight: 700;
    margin-bottom: 4px;
  }
  .ttl {
    line-height: 1.35;
    margin-bottom: 6px;
  }
  .nums {
    color: var(--muted);
    font-size: 11px;
  }

  .modal {
    position: fixed;
    inset: 0;
    background: #0008;
    display: grid;
    place-items: center;
    padding: 16px;
  }
  .panel {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 16px;
    width: min(420px, 100%);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  input {
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px;
    font: inherit;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  select {
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    margin-left: 4px;
  }
  .banners {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .banner {
    border-radius: 6px;
    padding: 8px 12px;
    font-weight: 700;
    border: 1px solid var(--border);
  }
  .banner.red {
    background: #3a1515;
    color: var(--down);
  }
  .banner.yellow {
    background: #3a3210;
    color: var(--stale);
  }
  .banner.orange {
    background: #3a2410;
    color: #f0a050;
  }
  .weights {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 8px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    min-height: 110px;
  }
  .wcol {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    height: 90px;
  }
  .wcol i {
    width: 18px;
    background: var(--live);
    border-radius: 3px 3px 0 0;
    min-height: 4px;
  }
  .heat {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(88px, 1fr));
    gap: 8px;
  }
  .hcell {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }
  .ev.soon {
    border-color: #c47a22;
  }
</style>
