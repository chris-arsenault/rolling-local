import { useEffect } from "react";
import truckPlaceholder from "./assets/food-truck-placeholder.svg";
import "./about.css";

const steps = [
  {
    title: "Register your truck",
    description: "Create an Ahara account and send us your truck’s name and cuisine. It takes a minute.",
  },
  {
    title: "We review it",
    description: "Every listing is checked before it goes public, so customers only see real trucks. You can check the decision on your registration page.",
  },
  {
    title: "Open the operator app",
    description: "Once approved, use the iOS or Android app to set your hours and service area, go live, and post a menu photo.",
  },
];

export default function About() {
  useEffect(() => { document.title = "For operators · Rolling Local"; }, []);
  return (
    <main className="about">
      <header className="about-header">
        <a className="brand" href="/">Rolling<span>Local</span></a>
        <nav aria-label="Site">
          <a href="/">Find a truck</a>
          <a href="/register">Register</a>
        </nav>
      </header>
      <section className="about-hero">
        <div>
          <p className="eyebrow">For food truck operators</p>
          <h1>Let your regulars find you. Then let everyone else.</h1>
          <p>Rolling Local shows hungry people where you’re parked and what you’re serving, right now. You tap to go live. We take care of the map.</p>
          <div className="about-actions">
            <a className="cta" href="/register">Register your truck</a>
            <a className="cta quiet" href="/">See the map</a>
          </div>
        </div>
        <img src={truckPlaceholder} alt="Food truck illustration. Photo coming soon." width="800" height="600" />
      </section>
      <section className="about-split" aria-label="What operators get">
        <article className="about-panel">
          <h2>You choose when to be found</h2>
          <ul>
            <li>Your location appears only during a sharing session and disappears when you stop or it expires.</li>
            <li>Your approximate location appears on the map while you’re sharing.</li>
            <li>Your listing stays searchable between stops, minus the location.</li>
          </ul>
        </article>
        <article className="about-panel dark">
          <h2>Your menu is always current</h2>
          <ul>
            <li>Snap a photo of today’s menu board and publish it from the app.</li>
            <li>Customers see when the photo was taken, so they know it’s fresh.</li>
            <li>No website to build and no menu to retype when the specials change.</li>
          </ul>
        </article>
      </section>
      <h2 className="about-section-title">From sign-up to first service</h2>
      <ol className="about-steps">
        {steps.map(({ title, description }) => (
          <li key={title}>
            <h3>{title}</h3>
            <p>{description}</p>
          </li>
        ))}
      </ol>
      <section className="about-closing">
        <h2>Ready to roll?</h2>
        <p>Your truck stays private until we approve it.</p>
        <a className="cta" href="/register">Register your truck</a>
      </section>
      <footer>An <a href="https://ahara.io">Ahara</a> product</footer>
    </main>
  );
}
