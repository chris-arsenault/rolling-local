import { useEffect } from "react";
import "./about.css";

const promises = [
  {
    title: "Share your truck with no effort.",
    description: "Tap once to go live, snap your menu, and your followers know where you are.",
  },
  {
    title: "Your privacy comes first.",
    description: "You decide when you’re visible. Your location is shown only while you share, and sharing stops automatically outside your area and hours.",
  },
  {
    title: "Nothing else. Really.",
    description: "No ads. No unwanted tracking. No offers. No upcharges.",
  },
];

export default function About() {
  useEffect(() => { document.title = "About · Rolling Local"; }, []);
  return (
    <main className="about">
      <header className="about-header">
        <a className="brand" href="/">Rolling<span>Local</span></a>
        <nav aria-label="Site">
          <a href="/">Find a truck</a>
          <a href="/register">Register</a>
        </nav>
      </header>
      {promises.map(({ title, description }) => (
        <section className="about-promise" key={title}>
          <h1>{title}</h1>
          <p>{description}</p>
        </section>
      ))}
      <section className="about-closing">
        <a className="cta" href="/register">Register your truck</a>
      </section>
      <footer>An <a href="https://ahara.io">Ahara</a> product</footer>
    </main>
  );
}
