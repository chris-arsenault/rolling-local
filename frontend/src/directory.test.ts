import { describe, expect, it } from "vitest";
import { eligible, filterDirectory, offline, type Truck } from "./directory";

const truck: Truck = {
  id: "truck-a",
  name: "Cañon Kitchen",
  cuisine: "Street tacos",
  active: true,
  location: {
    position: { latitude: 34.54, longitude: -112.47 },
    recordedAt: "2026-11-17T18:30:00Z",
    expiresAt: "2026-11-17T18:50:00Z",
  },
  menu: { url: "https://rolling-local.test/menu.jpg", capturedAt: "2026-11-17T18:00:00Z", publishedAt: "2026-11-17T18:01:00Z" },
};

describe("public discovery", () => {
  it("filters name and cuisine while keeping offline trucks searchable", () => {
    expect(filterDirectory([offline(truck)], "canon tacos").map((entry) => entry.id)).toEqual(["truck-a"]);
    expect(filterDirectory([truck], "ramen")).toEqual([]);
  });

  it("drops a position at its deadline", () => {
    const expires = Date.parse("2026-11-17T18:50:00Z");
    expect(eligible(truck, expires - 1)).toEqual(truck.location);
    expect(eligible(truck, expires)).toBeUndefined();
  });

  it("retains the menu and drops coordinates when updates fail", () => {
    expect(offline(truck)).not.toHaveProperty("location");
    expect(offline(truck).menu).toEqual(truck.menu);
  });
});
