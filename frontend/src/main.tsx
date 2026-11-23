import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { eligible, filterDirectory, loadDirectory, offline, type Truck } from "./directory";
import "./style.css";

const time = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const pollInterval = 20_000;
const TruckMap = lazy(async () => ({ default: (await import("./TruckMap")).TruckMap }));
const About = lazy(() => import("./About"));
const OperatorAccess = lazy(() => import("./OperatorAccess"));

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

function ago(iso: string, now: number): string {
  const minutes = Math.max(0, Math.round((now - Date.parse(iso)) / 60_000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return hours < 24 ? `${hours} hr ago` : time.format(new Date(iso));
}

function Status({ active }: { active: boolean }) {
  return <span className={active ? "status active" : "status"}>{active ? "Serving now" : "Not serving"}</span>;
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
          <p className="eyebrow">{truck.cuisine}</p>
          <h2 id="truck-name">{truck.name}</h2>
        </div>
        <button className="close" type="button" onClick={close} aria-label="Close truck details">×</button>
      </div>
      <div className="detail-status">
        <Status active={Boolean(position)} />
        {position
          ? <span>Location shared <time dateTime={position.recordedAt}>{ago(position.recordedAt, now)}</time></span>
          : <span>Location is only shown while the truck is serving.</span>}
      </div>
      <h3>Today’s menu</h3>
      {truck.menu ? (
        <figure>
          <img src={truck.menu.url} alt={`Menu photo for ${truck.name}`} />
          <figcaption>
            Photographed <time dateTime={truck.menu.capturedAt}>{time.format(new Date(truck.menu.capturedAt))}</time>
          </figcaption>
        </figure>
      ) : <p className="muted">No menu photo yet. Check back once the truck publishes one.</p>}
    </dialog>
  );
}

type Filter = "all" | "serving";

function App() {
  const { trucks, status, now } = useDirectory();
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"map" | "directory">("map");
  const [filter, setFilter] = useState<Filter>("all");
  const [selected, setSelected] = useState<string | null>(null);
  const select = useCallback((id: string) => setSelected(id), []);
  const matches = filterDirectory(trucks, query);
  const serving = matches.filter((truck) => eligible(truck, now));
  const results = filter === "serving" ? serving : matches;
  const detail = trucks.find((truck) => truck.id === selected);
  const directoryMessage = status === "loading" ? "Finding trucks…"
    : query.trim() ? "Nothing matches that search. Try a cuisine like “tacos” or “BBQ”."
    : filter === "serving" ? "No trucks are serving right now. Switch to All trucks to browse menus."
    : "No trucks are listed yet.";

  return (
    <main className="finder" data-mode={mode}>
      <aside className="sidebar">
        <header className="masthead">
          <a className="brand" href="/">Rolling<span>Local</span></a>
          <a className="masthead-link" href="/about">For operators</a>
        </header>
        <div className="lede">
          <h1 id="directory-title">Where’s the truck parked?</h1>
          <p>Live locations and menu photos from local food trucks, shared by the people cooking.</p>
        </div>
        <label className="search">
          <span className="screen-reader">Search trucks or cuisine</span>
          <input type="search" value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="Search by truck or cuisine" />
        </label>
        <div className="filters" role="group" aria-label="Show">
          <button type="button" aria-pressed={filter === "all"} onClick={() => setFilter("all")}>All trucks <b>{matches.length}</b></button>
          <button type="button" aria-pressed={filter === "serving"} onClick={() => setFilter("serving")}>Serving now <b>{serving.length}</b></button>
        </div>
        {status === "unavailable" && <p className="network" role="status">Live updates are unavailable. Showing the last directory we loaded, with locations hidden.</p>}
        <section className="directory" aria-labelledby="directory-title">
          {results.length === 0 ? <p className="empty">{directoryMessage}</p> : (
            <ul>
              {results.map((truck) => {
                const position = eligible(truck, now);
                return (
                  <li key={truck.id}>
                    <button type="button" className="truck" data-live={Boolean(position)} onClick={() => select(truck.id)}>
                      <span className="truck-main">
                        <strong>{truck.name}</strong>
                        <span className="cuisine">{truck.cuisine}</span>
                        <span className="truck-meta">
                          {position ? `Updated ${ago(position.recordedAt, now)}` : "Location hidden"}
                          {truck.menu && " · Menu photo"}
                        </span>
                      </span>
                      <Status active={Boolean(position)} />
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
        <footer className="sidebar-footer">
          <a href="/register">Run a food truck? List it here.</a>
          <span>An <a href="https://ahara.io">Ahara</a> product</span>
        </footer>
      </aside>
      <section className="map-panel" aria-label="Map">
        <Suspense fallback={<div className="map-frame" aria-label="Loading map" />}>
          <TruckMap trucks={results} now={now} onSelect={select} />
        </Suspense>
        <p className="map-summary">
          {status === "loading" ? "Loading locations…" : serving.length ? `${serving.length} ${serving.length === 1 ? "truck" : "trucks"} serving now` : "No trucks serving right now"}
        </p>
      </section>
      <nav className="mobile-nav" aria-label="Discovery views">
        <button type="button" aria-pressed={mode === "map"} onClick={() => setMode("map")}>Map</button>
        <button type="button" aria-pressed={mode === "directory"} onClick={() => setMode("directory")}>List</button>
      </nav>
      {detail && <TruckDetail truck={detail} now={now} close={() => setSelected(null)} />}
    </main>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Missing application root");

function Route() {
  switch (window.location.pathname.replace(/\/+$/, "")) {
    case "":
      return <App />;
    case "/about":
      return <About />;
    case "/register":
      return <OperatorAccess route="/register" />;
    case "/admin":
      return <OperatorAccess route="/admin" />;
    default:
      return <main className="page-loading">
        <h1>Page not found</h1>
        <a href="/">Find food trucks</a>
      </main>;
  }
}

createRoot(root).render(<Suspense fallback={<p className="page-loading" role="status">Loading…</p>}><Route /></Suspense>);
