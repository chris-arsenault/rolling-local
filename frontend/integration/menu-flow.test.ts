import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { filterDirectory, loadCatalog, loadDirectory } from "../src/directory";

afterEach(() => vi.unstubAllGlobals());

it("discovers a published and corrected dish through the real API response shapes", async () => {
  const output = execFileSync("cargo", [
    "test", "--locked", "--test", "api", "photo_to_worker_catalog_and_operator_correction",
    "--", "--exact", "--show-output",
  ], { cwd: fileURLToPath(new URL("../../backend/", import.meta.url)), encoding: "utf8" });
  const line = output.split("\n").find(line => line.startsWith("DISCOVERY_SAMPLE "));
  if (!line) throw new Error("The simulated publication did not return a catalog");
  const sample = JSON.parse(line.slice("DISCOVERY_SAMPLE ".length));
  vi.stubGlobal("window", { __APP_CONFIG__: { apiBaseUrl: "https://api.test" } });
  vi.stubGlobal("fetch", (url: URL) => {
    if (url.pathname === "/public/trucks") return Promise.resolve(Response.json(sample.directory));
    if (url.pathname === "/public/menu-catalog") return Promise.resolve(Response.json(sample.catalog));
    throw new Error(`Unexpected API request ${url.pathname}`);
  });
  const signal = new AbortController().signal;
  const [trucks, catalog] = await Promise.all([loadDirectory(signal), loadCatalog(signal)]);
  for (const query of ["sushi", "TUNA   Ro"]) {
    const matches = filterDirectory(trucks, query, catalog);
    expect(matches).toHaveLength(1);
    expect(matches[0]).toMatchObject({
      name: "Night Market", active: false, menu: { url: sample.catalog.menus[0].url },
      matchedItems: [{ name: "Tuna roll", priceText: "$11", description: null }],
    });
  }
});
