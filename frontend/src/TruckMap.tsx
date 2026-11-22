import maplibregl, { type Marker } from "maplibre-gl";
import { useEffect, useRef, useState } from "react";
import { eligible, type Truck } from "./directory";
import "maplibre-gl/dist/maplibre-gl.css";

type Props = { trucks: Truck[]; now: number; onSelect: (id: string) => void };

export function TruckMap({ trucks, now, onSelect }: Props) {
  const container = useRef<HTMLDivElement>(null);
  const map = useRef<maplibregl.Map | null>(null);
  const displayed = useRef(new Map<string, Marker>());
  const positioned = useRef(false);
  const [unavailable, setUnavailable] = useState(false);

  useEffect(() => {
    if (!container.current) return;
    try {
      const view = new maplibregl.Map({
        container: container.current,
        style: "https://tiles.openfreemap.org/styles/liberty",
        center: [-112.468, 34.54],
        zoom: 12,
        cooperativeGestures: true,
      });
      map.current = view;
      view.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-right");
      const resize = new ResizeObserver(() => view.resize());
      resize.observe(container.current);
      return () => {
        resize.disconnect();
        view.remove();
        map.current = null;
      };
    } catch {
      setUnavailable(true);
    }
  }, []);

  useEffect(() => {
    const view = map.current;
    if (!view) return;
    const visible = displayed.current;
    const next = new Set<string>();
    const bounds = new maplibregl.LngLatBounds();
    for (const truck of trucks) {
      const location = eligible(truck, now);
      if (!location) continue;
      const coords: [number, number] = [location.position.longitude, location.position.latitude];
      next.add(truck.id);
      bounds.extend(coords);
      const marker = visible.get(truck.id);
      if (marker) {
        marker.setLngLat(coords);
      } else {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "map-pin";
        button.setAttribute("aria-label", `View ${truck.name}`);
        button.title = truck.name;
        button.addEventListener("click", () => onSelect(truck.id));
        visible.set(truck.id, new maplibregl.Marker({ element: button }).setLngLat(coords).addTo(view));
      }
    }
    for (const [id, marker] of visible) {
      if (next.has(id)) continue;
      marker.remove();
      visible.delete(id);
    }
    if (!positioned.current && !bounds.isEmpty()) {
      view.fitBounds(bounds, { padding: 72, maxZoom: 13, duration: 0 });
      positioned.current = true;
    }
  }, [trucks, now, onSelect]);

  return (
    <div className="map-frame">
      <div ref={container} className="map" aria-label="Active truck map" />
      {unavailable && <p className="map-fallback">The map is unavailable in this browser. Browse the directory for menus and sharing status.</p>}
    </div>
  );
}
