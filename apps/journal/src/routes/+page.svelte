<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  type SierraRoot = {
    root: string;
    data_dir: string;
    journal_dir: string;
    scid_dir: string;
  };
  type Discovery = {
    primary: SierraRoot | null;
    extras: SierraRoot[];
  };

  let disc = $state<Discovery | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      disc = await invoke<Discovery>("sierra_discovery");
    } catch (e) {
      error = String(e);
    }
  });
</script>

<main>
  <h1>scdesk journal</h1>
  <p class="muted">NDJSON import lands in phase 2. Path discovery is live.</p>
  {#if error}
    <p class="err">{error}</p>
  {/if}
  {#if disc}
    {#if disc.primary}
      <section>
        <h2>primary Sierra Chart root</h2>
        <dl>
          <dt>root</dt><dd>{disc.primary.root}</dd>
          <dt>data</dt><dd>{disc.primary.data_dir}</dd>
          <dt>journal</dt><dd>{disc.primary.journal_dir}</dd>
          <dt>scid</dt><dd>{disc.primary.scid_dir}</dd>
        </dl>
      </section>
    {:else}
      <p class="err">No Sierra Chart folder found. Set SC_ROOT or install under ~/.wine/drive_c/SierraChart.</p>
    {/if}
    {#if disc.extras.length}
      <section>
        <h2>extra roots</h2>
        {#each disc.extras as r}
          <p>{r.root}</p>
        {/each}
      </section>
    {/if}
  {/if}
</main>

<style>
  :global(html, body) {
    margin: 0;
    background: #0b0f14;
    color: #e8edf5;
    font-family: ui-monospace, "JetBrains Mono", Menlo, monospace;
  }
  main {
    padding: 24px 28px;
    max-width: 900px;
  }
  h1 {
    letter-spacing: 0.08em;
    text-transform: uppercase;
    font-size: 16px;
  }
  h2 {
    font-size: 12px;
    color: #8b9bb4;
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }
  .muted {
    color: #8b9bb4;
  }
  .err {
    color: #f5c542;
  }
  dl {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: 6px 16px;
  }
  dt {
    color: #8b9bb4;
  }
  dd {
    margin: 0;
    word-break: break-all;
  }
</style>
