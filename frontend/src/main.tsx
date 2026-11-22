import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { eligible, filterDirectory, loadDirectory, offline, type Truck } from "./directory";
import "./style.css";

const time = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const pollInterval = 20_000;
const TruckMap = lazy(async () => ({ default: (await import("./TruckMap")).TruckMap }));

function useDirectory() {
  const [trucks, setTrucks] = useState<Truck[]>([]);
  const [status, setStatus] = useState<"loading" | "ready" | "unavailable">("loading");
  const [now, setNow] = useState(Date.now);

  useEffect(() => {
    let request: AbortController | undefined;
    const refresh = async () => {
      request?.abort();
      const controller = new AbortController();
      request = controller;
      try {
        const data = await loadDirectory(controller.signal);
        if (controller.signal.aborted) return;
        setTrucks(data);
        setStatus("ready");
      } catch {
        if (controller.signal.aborted) return;
        setTrucks((cached) => cached.map(offline));
        setStatus("unavailable");
      } finally {
        if (!controller.signal.aborted) setNow(Date.now());
      }
    };
    const resumed = () => {
      setNow(Date.now());
      if (!document.hidden) void refresh();
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), pollInterval);
    document.addEventListener("visibilitychange", resumed);
    return () => {
      request?.abort();
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", resumed);
    };
  }, []);

  useEffect(() => {
    const expiries = trucks.flatMap((truck) => {
      const location = eligible(truck, now);
      return location ? [Date.parse(location.expiresAt)] : [];
    });
    if (expiries.length === 0) return;
    const timer = window.setTimeout(() => setNow(Date.now()), Math.max(0, Math.min(...expiries) - Date.now()));
    return () => window.clearTimeout(timer);
  }, [trucks, now]);

  return { trucks, status, now };
}

function Status({ active }: { active: boolean }) {
  return <span className={active ? "status active" : "status"}>{active ? "Sharing" : "Offline"}</span>;
}

function TruckDetail({ truck, now, close }: { truck: Truck; now: number; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    element.showModal();
    return () => element.close();
  }, []);
  const position = eligible(truck, now);
  return (
    <dialog ref={dialog} onCancel={close} aria-labelledby="truck-name">
      <div className="detail-top">
        <div>
          <h2 id="truck-name">{truck.name}</h2>
          <span className="cuisine">{truck.cuisine}</span>
        </div>
        <button className="close" type="button" onClick={close} aria-label="Close truck details">×</button>
      </div>
      <div className="detail-status">
        <Status active={Boolean(position)} />
        {position && <span>Location updated <time dateTime={position.recordedAt}>{time.format(new Date(position.recordedAt))}</time></span>}
      </div>
      <h3>Menu</h3>
      {truck.menu ? (
        <figure>
          <img src={truck.menu.url} alt={`Menu photo for ${truck.name}`} />
          <figcaption>Photo taken <time dateTime={truck.menu.capturedAt}>{time.format(new Date(truck.menu.capturedAt))}</time></figcaption>
        </figure>
      ) : <p className="cuisine">This truck hasn’t published a menu photo.</p>}
    </dialog>
  );
}

function App() {
  const { trucks, status, now } = useDirectory();
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"map" | "directory">("map");
  const [selected, setSelected] = useState<string | null>(null);
  const select = useCallback((id: string) => setSelected(id), []);
  const results = filterDirectory(trucks, query);
  const active = results.filter((truck) => eligible(truck, now)).length;
  const detail = trucks.find((truck) => truck.id === selected);
  const directoryMessage = status === "loading" ? "Loading trucks…" : query.trim() ? "No trucks match your search." : "No trucks are listed yet.";

  return (
    <main>
      <header className="header">
        <div className="brand">Rolling<span>Local</span></div>
        <label className="search">
          <span className="screen-reader">Search trucks or cuisine</span>
          <input type="search" value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="Search trucks or cuisine" />
        </label>
      </header>
      <nav className="mobile-nav" aria-label="Discovery views">
        <button type="button" aria-pressed={mode === "map"} onClick={() => setMode("map")}>Map</button>
        <button type="button" aria-pressed={mode === "directory"} onClick={() => setMode("directory")}>Directory</button>
      </nav>
      {status === "unavailable" && <p className="network" role="status">Live updates are unavailable. Showing any previously loaded directory entries offline.</p>}
      <div className="discovery" data-mode={mode}>
        <section className="map-panel" aria-label="Map">
          <div className="map-summary">
            <span className={active > 0 ? "status active" : "status"}>
              {status === "loading" ? "Loading locations" : active ? `${active} sharing` : "No active locations"}
            </span>
          </div>
          <Suspense fallback={<div className="map-frame" aria-label="Loading map" />}>
            <TruckMap trucks={results} now={now} onSelect={select} />
          </Suspense>
        </section>
        <section className="directory" aria-labelledby="directory-title">
          <div className="directory-title">
            <h1 id="directory-title">Food trucks</h1>
            <span>{results.length} {results.length === 1 ? "truck" : "trucks"}</span>
          </div>
          {results.length === 0 ? <p className="empty">{directoryMessage}</p> : (
            <ul>
              {results.map((truck) => (
                <li key={truck.id}>
                  <button type="button" className="truck" onClick={() => select(truck.id)}>
                    <span className="truck-main">
                      <strong>{truck.name}</strong>
                      <span className="cuisine">{truck.cuisine}</span>
                    </span>
                    <Status active={Boolean(eligible(truck, now))} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
      {detail && <TruckDetail truck={detail} now={now} close={() => setSelected(null)} />}
    </main>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Missing application root");
createRoot(root).render(<App />);
