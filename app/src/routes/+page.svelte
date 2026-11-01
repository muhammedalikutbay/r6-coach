<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // Temporary test match; real screens come with a later task.
  const TEST_MATCH =
    "C:\Users\Kutbay\Desktop\R6Koc-veri\MatchReplay\Match-2026-10-31_00-54-47-11844";

  let status = $state("");

  async function parseTest() {
    status = "Parsing...";
    try {
      const result = await invoke<{ rounds?: unknown[] }>("parse_match", { path: TEST_MATCH });
      console.log("parse_match result:", result);
      status = `OK: ${result.rounds?.length ?? "?"} rounds`;
    } catch (e) {
      console.error("parse_match failed:", e);
      status = `Error: ${e}`;
    }
  }
</script>

<main>
  <h1>R6 Coach</h1>
  <button onclick={parseTest}>Parse test match</button>
  <p>{status}</p>
</main>

<style>
  main { font-family: system-ui, sans-serif; padding: 2rem; }
</style>
