import { useEffect } from "react";
import truckPlaceholder from "./assets/food-truck-placeholder.svg";
import "./about.css";

const features = [
  {
    title: "Go live when you’re serving",
    description: "Start sharing from your phone’s home screen. Your approximate location appears on the map during your sharing session and disappears when you stop or the session expires.",
  },
  {
    title: "Show what’s on the menu",
    description: "Take a menu photo and publish it from the operator app. The timestamp helps customers tell how recent it is.",
  },
  {
    title: "Stay discoverable between stops",
    description: "Once approved, your truck is searchable by name and cuisine even when you’re offline. Customers can see your listing and menu while your location stays private.",
  },
];

export default function About() {
  useEffect(() => { document.title = "About · Rolling Local"; }, []);
  return (
    <main className="about">
      <header className="about-header">
        <a className="brand" href="/">Rolling<span>Local</span></a>
        <a href="/">Find food trucks</a>
      </header>
      <section className="about-hero">
        <div>
          <h1>Help customers find your food truck.</h1>
          <p>Share where you’re serving and what’s on the menu. Rolling Local puts both in one place for customers nearby.</p>
        </div>
        <img src={truckPlaceholder} alt="Food truck illustration. Photo coming soon." width="800" height="600" />
      </section>
      <section className="about-features" aria-label="Features for truck operators">
        {features.map(({ title, description }) => (
          <article key={title}>
            <h2>{title}</h2>
            <p>{description}</p>
          </article>
        ))}
      </section>
      <section className="about-review">
        <h2>From application to first service</h2>
        <p>We review your truck’s registration before it appears in the public directory. Once it’s approved, use the iOS or Android operator app to set your operating hours and area, share your location, and publish a menu photo.</p>
      </section>
      <footer>An <a href="https://ahara.io">Ahara</a> product</footer>
    </main>
  );
}
