<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  type Fill = { datetime: string; price: number; qty: number; side: string };
  type Trade = {
    id: string;
    account: string;
    symbol_raw: string;
    symbol_root: string;
    direction: string;
    qty: number;
    entry_price: number;
    exit_price: number | null;
    stop_price: number | null;
    net_pnl: number;
    r_value: number | null;
    mfe: number | null;
    mae: number | null;
    duration_seconds: number | null;
    open_datetime: string;
    close_datetime: string | null;
    trading_day: string;
    is_closed: boolean;
    is_sim: boolean;
    notes: string;
    tags: string[];
    screenshots: { path: string; crop?: { x: number; y: number; w: number; h: number } | null }[];
    fills: Fill[];
    mae_source?: string | null;
    post_exit_mfe?: number | null;
    checklist: { id: string; label: string; checked: boolean }[];
  };
  type Shot = Trade["screenshots"][number];
  type Prop = {
    account: string;
    equity: number;
    buffer: number;
    target_remaining: number;
    peak: number;
  };
  type Kpis = {
    trades: number;
    wins: number;
    losses: number;
    days: number;
    net_pnl: number;
    net_r: number;
    win_rate: number;
    profit_factor: number;
    expectancy: number;
    avg_win: number;
    avg_loss: number;
    max_dd: number;
    avg_r: number;
  };
  type Eq = { ts: number; equity: number; r_equity: number };
  type Day = { date: string; pnl: number; r: number; trades: number };
  type Mc = { runs: number; p05: number; p50: number; p95: number; mean: number };
  type Settings = {
    exclude_sim: boolean;
    default_risk_ticks: number;
    unit: string;
    rules: { max_trades_per_day: number; max_daily_loss: number; max_daily_loss_r: number };
  };
  type PropSpec = {
    account: string;
    starting_balance: number;
    dd_type: string;
    dd_value: number;
    profit_target: number;
  };
  type Session = { date: string; notes: string; mood: number | null; market_condition: string };
  type Break = { date: string; kind: string; text: string };

  const emptyFilter = () => ({
    accounts: [] as string[],
    roots: [] as string[],
    direction: "",
    exclude_sim: false,
    closed_only: true,
    query: "",
  });

  let tab = $state("dashboard");
  let filter = $state(emptyFilter());
  let settings = $state<Settings | null>(null);
  let trades = $state<Trade[]>([]);
  let kpis = $state<Kpis | null>(null);
  let equity = $state<Eq[]>([]);
  let days = $state<Day[]>([]);
  let hours = $state<[number, number, number][]>([]);
  let mc = $state<Mc | null>(null);
  let accts = $state<string[]>([]);
  let selected = $state<Trade | null>(null);
  let noteDraft = $state("");
  let tagDraft = $state("");
  let session = $state<Session>({ date: "", notes: "", mood: null, market_condition: "" });
  let breaks = $state<Break[]>([]);
  let gallery = $state<Trade[]>([]);
  let dd = $state<Eq[]>([]);
  let rhist = $state<[number, number][]>([]);
  let scatter = $state<[number, number, number][]>([]);
  let props = $state<Prop[]>([]);
  let propDraft = $state<PropSpec>({
    account: "",
    starting_balance: 50000,
    dd_type: "static",
    dd_value: 2000,
    profit_target: 3000,
  });
  let error = $state<string | null>(null);
  let importing = $state(false);
  let tsvDraft = $state("");

  const unit = $derived(settings?.unit === "R" ? "R" : "$");
  function money(n: number | null | undefined): string {
    if (n == null || Number.isNaN(n)) return "—";
    if (unit === "R") return `${n.toFixed(2)}R`;
    return n.toLocaleString("en-US", { maximumFractionDigits: 0, signDisplay: "exceptZero" });
  }
  function val(t: Trade): number {
    return unit === "R" ? t.r_value ?? 0 : t.net_pnl;
  }
  function cls(n: number): string {
    if (n > 0) return "up";
    if (n < 0) return "down";
    return "";
  }
  function spark(pts: Eq[]): string {
    if (pts.length < 2) return "";
    const key = unit === "R" ? "r_equity" : "equity";
    const ys = pts.map((p) => p[key]);
    const min = Math.min(...ys);
    const max = Math.max(...ys);
    const span = Math.max(1, max - min);
    return pts
      .map((p, i) => {
        const x = (i / (pts.length - 1)) * 520;
        const y = 80 - ((p[key] - min) / span) * 80;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  }

  async function refresh() {
    try {
      const f = {
        ...filter,
        direction: filter.direction || null,
        exclude_sim: settings?.exclude_sim ?? false,
      };
      const [t, k, e, c, h, m, a, b, g, ddown, rh, sc, pr] = await Promise.all([
        invoke<Trade[]>("list_trades", { filter: f }),
        invoke<Kpis>("kpis", { filter: f }),
        invoke<Eq[]>("equity", { filter: f }),
        invoke<Day[]>("calendar", { filter: f }),
        invoke<[number, number, number][]>("hours", { filter: f }),
        invoke<Mc>("monte", { filter: f }),
        invoke<string[]>("accounts"),
        invoke<Break[]>("rule_breaks", { filter: f }),
        invoke<Trade[]>("gallery", { filter: f }),
        invoke<Eq[]>("drawdown", { filter: f }),
        invoke<[number, number][]>("r_hist", { filter: f }),
        invoke<[number, number, number][]>("mfe_mae", { filter: f }),
        invoke<Prop[]>("prop_tiles", { filter: f }),
      ]);
      trades = t;
      kpis = k;
      equity = e;
      days = c;
      hours = h;
      mc = m;
      accts = a;
      breaks = b;
      gallery = g;
      dd = ddown;
      rhist = rh;
      scatter = sc;
      props = pr;
      void invoke("write_halt", { breaks: b });
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function doImport() {
    importing = true;
    try {
      const n = await invoke<number>("import_journal");
      error = n ? `imported ${n} rows` : "no NDJSON found";
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      importing = false;
    }
  }

  async function importTsv() {
    try {
      const n = await invoke<number>("import_tradeslist", { text: tsvDraft });
      tsvDraft = "";
      error = `tradeslist ${n} rows`;
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function pick(t: Trade) {
    selected = t;
    noteDraft = t.notes;
    tagDraft = t.tags.join(", ");
    tab = "trades";
  }

  async function saveNotes() {
    if (!selected) return;
    await invoke("save_notes", { id: selected.id, notes: noteDraft });
    const tags = tagDraft
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    await invoke("save_tags", { id: selected.id, tags });
    await refresh();
    selected = trades.find((x) => x.id === selected?.id) ?? selected;
  }

  async function remove(id: string) {
    await invoke("delete_trade", { id });
    selected = null;
    await refresh();
  }

  async function persistSettings(partial: Partial<Settings>) {
    if (!settings) return;
    settings = await invoke<Settings>("save_settings", { settings: { ...settings, ...partial } });
    await refresh();
  }

  async function loadSession(date: string) {
    session = await invoke<Session>("get_session", { date });
    tab = "diary";
  }

  async function saveSess() {
    await invoke("save_session", { session });
  }

  async function onPaste(ev: ClipboardEvent) {
    if (!selected) return;
    const item = [...(ev.clipboardData?.items ?? [])].find((i) => i.type.startsWith("image/"));
    if (!item) return;
    ev.preventDefault();
    const file = item.getAsFile();
    if (!file) return;
    const buf = await file.arrayBuffer();
    const bytes = new Uint8Array(buf);
    let bin = "";
    bytes.forEach((b) => (bin += String.fromCharCode(b)));
    const b64 = btoa(bin);
    const shots = await invoke<Shot[]>("attach_screenshot", {
      id: selected.id,
      base64Png: b64,
    });
    selected = { ...selected, screenshots: shots };
    await refresh();
  }

  const defaultChecks = [
    { id: "htf", label: "HTF aligned", checked: false },
    { id: "news", label: "News clear", checked: false },
    { id: "risk", label: "Risk defined", checked: false },
    { id: "aplus", label: "A+ setup", checked: false },
  ];

  async function scanTicks() {
    if (!selected) return;
    try {
      const r = await invoke<unknown>("scan_scid", { id: selected.id });
      error = r ? "scid MFE/MAE applied" : "no matching .scid";
      await refresh();
      selected = trades.find((x) => x.id === selected?.id) ?? selected;
    } catch (e) {
      error = String(e);
    }
  }

  async function replay() {
    if (!selected) return;
    await invoke("write_replay", {
      symbol: selected.symbol_raw,
      datetime: selected.open_datetime,
    });
    error = "wrote Data/scdesk/replay.json";
  }

  async function saveChecks() {
    if (!selected) return;
    const items = selected.checklist.length ? selected.checklist : defaultChecks;
    await invoke("set_checklist", { id: selected.id, items });
    await refresh();
  }

  async function moveShot(i: number, dir: number) {
    if (!selected) return;
    const j = i + dir;
    if (j < 0 || j >= selected.screenshots.length) return;
    const shots = [...selected.screenshots];
    [shots[i], shots[j]] = [shots[j], shots[i]];
    await invoke("set_shots", { id: selected.id, shots });
    selected = { ...selected, screenshots: shots };
  }

  function heatCell(d: Day | null): string {
    if (!d) return "";
    const v = unit === "R" ? d.r : d.pnl;
    if (v > 0) return "up";
    if (v < 0) return "down";
    return "";
  }

  const yearCells = $derived.by(() => {
    if (!days.length) return [] as { date: string; d: Day | null }[];
    const map = new Map(days.map((d) => [d.date, d]));
    const start = Date.parse(`${days[0].date}T00:00:00Z`);
    const end = Date.parse(`${days[days.length - 1].date}T00:00:00Z`);
    if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return [];
    const out: { date: string; d: Day | null }[] = [];
    for (let t = start; t <= end; t += 86_400_000) {
      const date = new Date(t).toISOString().slice(0, 10);
      out.push({ date, d: map.get(date) ?? null });
    }
    return out;
  });

  const scatterPts = $derived.by(() => {
    if (!scatter.length) return [] as { x: number; y: number; pnl: number }[];
    const xs = scatter.map((p) => p[0]);
    const ys = scatter.map((p) => p[1]);
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);
    const sx = maxX - minX || 1;
    const sy = maxY - minY || 1;
    return scatter.map(([mae, mfe, pnl]) => ({
      x: 8 + ((mae - minX) / sx) * 184,
      y: 72 - ((mfe - minY) / sy) * 64,
      pnl,
    }));
  });

  async function saveProp() {
    if (!propDraft.account) return;
    await invoke("save_prop", { spec: propDraft });
    await refresh();
  }

  onMount(() => {
    void (async () => {
      settings = await invoke<Settings>("get_settings");
      await doImport();
    })();
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  });
</script>

<div class="desk">
  <header>
    <div class="brand">scdesk journal</div>
    <nav>
      {#each ["dashboard", "trades", "calendar", "gallery", "edge", "diary", "rules", "settings"] as t}
        <button class:on={tab === t} onclick={() => (tab = t)}>{t}</button>
      {/each}
    </nav>
    <div class="spacer"></div>
    <button class:on={unit === "$"} onclick={() => persistSettings({ unit: "$" })}>$</button>
    <button class:on={unit === "R"} onclick={() => persistSettings({ unit: "R" })}>R</button>
    <button onclick={doImport} disabled={importing}>{importing ? "import…" : "import"}</button>
  </header>
  {#if error}<div class="err">{error}</div>{/if}

  {#if tab === "dashboard" && kpis}
    <section class="kpis">
      <article><div class="k">net</div><b class={cls(kpis.net_pnl)}>{unit === "R" ? money(kpis.net_r) : money(kpis.net_pnl)}</b></article>
      <article><div class="k">win%</div><b>{kpis.win_rate.toFixed(1)}</b></article>
      <article><div class="k">PF</div><b>{kpis.profit_factor.toFixed(2)}</b></article>
      <article><div class="k">expect</div><b class={cls(kpis.expectancy)}>{money(kpis.expectancy)}</b></article>
      <article><div class="k">max DD</div><b class="down">{money(kpis.max_dd)}</b></article>
      <article><div class="k">trades</div><b>{kpis.trades}</b></article>
      <article><div class="k">days</div><b>{kpis.days}</b></article>
      <article><div class="k">avg R</div><b>{kpis.avg_r.toFixed(2)}</b></article>
    </section>
    <section class="panel">
      <h2>equity</h2>
      {#if equity.length > 1}
        <svg viewBox="0 0 520 80" class="spark">
          <polyline fill="none" stroke="#3ddc97" stroke-width="1.6" points={spark(equity)} />
        </svg>
      {:else}
        <p class="muted">import trades to build a curve</p>
      {/if}
    </section>
    {#if props.length}
      <section class="kpis">
        {#each props as p}
          <article>
            <div class="k">{p.account} buffer</div>
            <b class={cls(p.buffer)}>{money(p.buffer)}</b>
            <div class="k">target left {money(p.target_remaining)}</div>
          </article>
        {/each}
      </section>
    {/if}
    {#if mc}
      <section class="panel">
        <h2>monte carlo (400 shuffles of R)</h2>
        <div class="kpis">
          <article><div class="k">p5</div><b>{mc.p05.toFixed(1)}R</b></article>
          <article><div class="k">p50</div><b>{mc.p50.toFixed(1)}R</b></article>
          <article><div class="k">p95</div><b>{mc.p95.toFixed(1)}R</b></article>
          <article><div class="k">mean</div><b>{mc.mean.toFixed(1)}R</b></article>
        </div>
      </section>
    {/if}
    {#if dd.length > 1}
      <section class="panel">
        <h2>drawdown</h2>
        <svg viewBox="0 0 520 80" class="spark">
          <polyline fill="none" stroke="#ff5c5c" stroke-width="1.6" points={spark(dd.map((p) => ({ ...p, r_equity: p.equity })))} />
        </svg>
      </section>
    {/if}
    {#if rhist.length}
      <section class="panel">
        <h2>R distribution</h2>
        <div class="hours">
          {#each rhist as [x, n]}
            <div class="hrow">
              <span>{x.toFixed(1)}</span>
              <i style="width: {Math.min(100, n * 8)}%"></i>
              <b>{n}</b>
            </div>
          {/each}
        </div>
      </section>
    {/if}
    {#if scatter.length}
      <section class="panel">
        <h2>MAE vs MFE (price)</h2>
        <svg viewBox="0 0 200 80" class="spark">
          {#each scatterPts as p}
            <circle cx={p.x} cy={p.y} r="2.2" fill={p.pnl >= 0 ? "#3ddc97" : "#ff5c5c"} />
          {/each}
        </svg>
      </section>
    {/if}
    {#if breaks.length}
      <section class="panel">
        <h2>rule breaks</h2>
        {#each breaks as b}
          <div class="break">{b.date} · {b.text}</div>
        {/each}
      </section>
    {/if}
  {/if}

  {#if tab === "trades"}
    <div class="split">
      <div class="list">
        <div class="filters">
          <input placeholder="search" bind:value={filter.query} onchange={refresh} />
          <select bind:value={filter.direction} onchange={refresh}>
            <option value="">all</option>
            <option value="LONG">LONG</option>
            <option value="SHORT">SHORT</option>
          </select>
          <select
            onchange={(e) => {
              const v = e.currentTarget.value;
              filter.accounts = v ? [v] : [];
              void refresh();
            }}
          >
            <option value="">all accounts</option>
            {#each accts as a}
              <option value={a}>{a}</option>
            {/each}
          </select>
        </div>
        <table>
          <thead>
            <tr><th>when</th><th>sym</th><th>dir</th><th>qty</th><th>pnl</th></tr>
          </thead>
          <tbody>
            {#each trades as t}
              <tr class:sel={selected?.id === t.id} onclick={() => pick(t)}>
                <td>{t.trading_day}</td>
                <td>{t.symbol_root}</td>
                <td class={t.direction === "LONG" ? "up" : "down"}>{t.direction}</td>
                <td>{t.qty}</td>
                <td class={cls(val(t))}>{money(val(t))}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div class="detail panel">
        {#if selected}
          <h2>{selected.symbol_raw} · {selected.account}</h2>
          <dl>
            <dt>dir</dt><dd class={selected.direction === "LONG" ? "up" : "down"}>{selected.direction} × {selected.qty}</dd>
            <dt>entry</dt><dd>{selected.entry_price}</dd>
            <dt>exit</dt><dd>{selected.exit_price ?? "open"}</dd>
            <dt>stop</dt><dd>{selected.stop_price ?? "—"}</dd>
            <dt>net</dt><dd class={cls(selected.net_pnl)}>{money(selected.net_pnl)}</dd>
            <dt>R</dt><dd>{selected.r_value?.toFixed(2) ?? "—"}</dd>
            <dt>MFE/MAE</dt><dd>{selected.mfe?.toFixed(2) ?? "—"} / {selected.mae?.toFixed(2) ?? "—"} {selected.mae_source ?? ""}</dd>
            <dt>post MFE</dt><dd>{selected.post_exit_mfe?.toFixed(2) ?? "—"}</dd>
            <dt>dur</dt><dd>{selected.duration_seconds ?? "—"}s</dd>
          </dl>
          <div class="row">
            <button onclick={scanTicks}>.scid MFE</button>
            <button onclick={replay}>replay cmd</button>
          </div>
          <h3>fills</h3>
          <ul>
            {#each selected.fills as f}
              <li>{f.side} {f.qty} @ {f.price} · {f.datetime}</li>
            {/each}
          </ul>
          <label class="muted">notes (paste image for screenshot)
            <textarea bind:value={noteDraft}></textarea>
          </label>
          <label class="muted">tags
            <input bind:value={tagDraft} placeholder="setup, fade, news" />
          </label>
          <div class="row">
            <button onclick={saveNotes}>save</button>
            <button onclick={() => selected && remove(selected.id)}>delete</button>
          </div>
          <h3>checklist</h3>
          {#each (selected.checklist.length ? selected.checklist : defaultChecks) as c, i}
            <label class="muted"
              ><input
                type="checkbox"
                checked={c.checked}
                onchange={(e) => {
                  const items = (selected?.checklist.length ? selected.checklist : defaultChecks).map((x, j) =>
                    j === i ? { ...x, checked: e.currentTarget.checked } : x,
                  );
                  if (selected) selected = { ...selected, checklist: items };
                }}
              /> {c.label}</label
            >
          {/each}
          <button onclick={saveChecks}>save checklist</button>
          {#if selected.screenshots.length}
            <div class="shots">
              {#each selected.screenshots as s, i}
                <div class="shotrow">
                  <span class="muted">{s.path}</span>
                  <button onclick={() => moveShot(i, -1)}>up</button>
                  <button onclick={() => moveShot(i, 1)}>dn</button>
                </div>
              {/each}
            </div>
          {/if}
        {:else}
          <p class="muted">select a trade</p>
        {/if}
      </div>
    </div>
  {/if}

  {#if tab === "calendar"}
    <div class="year">
      {#each yearCells as c}
        <i
          class={heatCell(c.d)}
          title={c.d ? `${c.date} ${c.d.trades} ${money(unit === "R" ? c.d.r : c.d.pnl)}` : c.date}
        ></i>
      {/each}
    </div>
    <div class="cal">
      {#each days as d}
        <button
          class="day {cls(unit === 'R' ? d.r : d.pnl)}"
          onclick={() => loadSession(d.date)}
          title="{d.trades} trades"
        >
          <b>{d.date.slice(5)}</b>
          <span>{money(unit === "R" ? d.r : d.pnl)}</span>
        </button>
      {/each}
    </div>
    <section class="panel">
      <h2>R by hour (UTC)</h2>
      <div class="hours">
        {#each hours as [h, r, n]}
          {#if n}
            <div class="hrow">
              <span>{String(h).padStart(2, "0")}</span>
              <i style="width: {Math.min(100, Math.abs(r) * 8)}%" class={cls(r)}></i>
              <b class={cls(r)}>{r.toFixed(1)}R · {n}</b>
            </div>
          {/if}
        {/each}
      </div>
    </section>
  {/if}

  {#if tab === "gallery"}
    <div class="gallery">
      {#each gallery as t}
        <button type="button" class="gal" onclick={() => pick(t)}>
          <div class="k">{t.symbol_root} · {t.trading_day}</div>
          <div class={cls(val(t))}>{money(val(t))}</div>
          <div class="muted">{t.screenshots.length} shot(s)</div>
        </button>
      {:else}
        <p class="muted">paste an image on a trade to fill the gallery</p>
      {/each}
    </div>
  {/if}

  {#if tab === "edge"}
    <section class="panel">
      <h2>saved view (edge)</h2>
      <p class="muted">filter the table, then this tab restates the same KPIs for that slice.</p>
      <div class="filters">
        <input placeholder="query" bind:value={filter.query} onchange={refresh} />
        <input
          placeholder="root e.g. NQ"
          onchange={(e) => {
            const v = e.currentTarget.value.trim().toUpperCase();
            filter.roots = v ? [v] : [];
            void refresh();
          }}
        />
      </div>
      {#if kpis}
        <p>{kpis.trades} trades · PF {kpis.profit_factor.toFixed(2)} · win {kpis.win_rate.toFixed(1)}% · net {money(unit === "R" ? kpis.net_r : kpis.net_pnl)}</p>
      {/if}
    </section>
  {/if}

  {#if tab === "diary"}
    <section class="panel">
      <h2>session {session.date || "(pick a calendar day)"}</h2>
      <input placeholder="YYYY-MM-DD" bind:value={session.date} onchange={() => session.date && loadSession(session.date)} />
      <input placeholder="market condition" bind:value={session.market_condition} />
      <label class="muted">mood 1–5
        <input type="number" min="1" max="5" bind:value={session.mood} />
      </label>
      <textarea bind:value={session.notes} placeholder="what worked, what didn't"></textarea>
      <button onclick={saveSess}>save session</button>
    </section>
  {/if}

  {#if tab === "rules" && settings}
    <section class="panel">
      <h2>risk rules</h2>
      <label class="muted">max trades / day (0 = off)
        <input
          type="number"
          value={settings.rules.max_trades_per_day}
          onchange={(e) =>
            persistSettings({
              rules: { ...settings!.rules, max_trades_per_day: Number(e.currentTarget.value) },
            })}
        />
      </label>
      <label class="muted">max daily loss $
        <input
          type="number"
          value={settings.rules.max_daily_loss}
          onchange={(e) =>
            persistSettings({
              rules: { ...settings!.rules, max_daily_loss: Number(e.currentTarget.value) },
            })}
        />
      </label>
      <label class="muted">max daily loss R
        <input
          type="number"
          value={settings.rules.max_daily_loss_r}
          onchange={(e) =>
            persistSettings({
              rules: { ...settings!.rules, max_daily_loss_r: Number(e.currentTarget.value) },
            })}
        />
      </label>
      {#each breaks as b}
        <div class="break">{b.date} · {b.text}</div>
      {:else}
        <p class="muted">no breaks in the current filter</p>
      {/each}
    </section>
  {/if}

  {#if tab === "settings" && settings}
    <section class="panel">
      <h2>settings</h2>
      <label class="muted"
        ><input
          type="checkbox"
          checked={settings.exclude_sim}
          onchange={(e) => persistSettings({ exclude_sim: e.currentTarget.checked })}
        /> exclude sim from stats</label
      >
      <label class="muted">default risk ticks (when no stop)
        <input
          type="number"
          value={settings.default_risk_ticks}
          onchange={(e) => persistSettings({ default_risk_ticks: Number(e.currentTarget.value) })}
        />
      </label>
      <h3>import TradesList TSV</h3>
      <textarea bind:value={tsvDraft} placeholder="paste Sierra TradesList export"></textarea>
      <button onclick={importTsv}>import TSV</button>
      <h3>prop firm account</h3>
      {#if props.length}
        <ul>
          {#each props as p}
            <li>{p.account} · eq {money(p.equity)} · buffer {money(p.buffer)}</li>
          {/each}
        </ul>
      {/if}
      <input placeholder="account" bind:value={propDraft.account} />
      <label class="muted">starting
        <input type="number" bind:value={propDraft.starting_balance} />
      </label>
      <label class="muted">dd type
        <select bind:value={propDraft.dd_type}><option value="static">static</option><option value="trailing">trailing</option></select>
      </label>
      <label class="muted">dd value
        <input type="number" bind:value={propDraft.dd_value} />
      </label>
      <label class="muted">profit target
        <input type="number" bind:value={propDraft.profit_target} />
      </label>
      <button onclick={saveProp}>save prop</button>
    </section>
  {/if}
</div>

<style>
  .desk { height: 100%; overflow: auto; padding: 14px 16px 24px; display: flex; flex-direction: column; gap: 14px; }
  header { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; position: sticky; top: 0; background: color-mix(in srgb, var(--bg) 92%, transparent); padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .brand { letter-spacing: 0.12em; text-transform: uppercase; font-size: 11px; color: var(--muted); }
  nav { display: flex; flex-wrap: wrap; gap: 4px; }
  .spacer { flex: 1; }
  .err { color: var(--stale); }
  .muted { color: var(--muted); font-size: 12px; }
  .kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr)); gap: 8px; }
  .kpis article, .panel { background: var(--panel); border: 1px solid var(--border); border-radius: 8px; padding: 12px; }
  .k { color: var(--muted); font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; }
  .up { color: var(--up); }
  .down { color: var(--down); }
  h2 { margin: 0 0 8px; font-size: 11px; letter-spacing: 0.12em; text-transform: uppercase; color: var(--muted); }
  .spark { width: 100%; height: 80px; }
  .split { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(280px, 0.8fr); gap: 12px; min-height: 0; }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid var(--border); }
  tr { cursor: pointer; }
  tr.sel { background: var(--panel-2); }
  .filters { display: flex; gap: 6px; margin-bottom: 8px; flex-wrap: wrap; }
  dl { display: grid; grid-template-columns: 80px 1fr; gap: 4px 10px; }
  dt { color: var(--muted); }
  dd { margin: 0; }
  .row { display: flex; gap: 8px; margin-top: 8px; }
  .cal { display: grid; grid-template-columns: repeat(auto-fill, minmax(88px, 1fr)); gap: 6px; }
  .day { display: flex; flex-direction: column; gap: 4px; padding: 8px; text-align: left; }
  .hours { display: flex; flex-direction: column; gap: 4px; }
  .hrow { display: grid; grid-template-columns: 28px 1fr 90px; gap: 8px; align-items: center; }
  .hrow i { display: block; height: 8px; background: var(--up); border-radius: 2px; }
  .hrow i.down { background: var(--down); }
  .gallery { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 8px; }
  .gallery .gal { background: var(--panel); border: 1px solid var(--border); border-radius: 8px; padding: 12px; text-align: left; display: flex; flex-direction: column; gap: 4px; }
  .break { color: var(--down); margin: 4px 0; }
  .year { display: flex; flex-wrap: wrap; gap: 3px; }
  .year i { width: 10px; height: 10px; background: var(--panel-2); border-radius: 2px; }
  .year i.up { background: var(--up); }
  .year i.down { background: var(--down); }
  .shotrow { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  label { display: flex; flex-direction: column; gap: 4px; margin: 8px 0; }
</style>
