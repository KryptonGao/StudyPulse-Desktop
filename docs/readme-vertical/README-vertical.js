/* global document, URLSearchParams, window */

(function () {
  const slides = Array.from(document.querySelectorAll(".slide"));
  const controls = document.querySelector(".deck-controls");
  const requested = Number(new URLSearchParams(window.location.search).get("slide"));
  let current = Number.isInteger(requested) && requested >= 1 && requested <= slides.length ? requested : 1;

  function show(index, updateUrl) {
    current = Math.min(Math.max(index, 1), slides.length);
    slides.forEach((slide, offset) => {
      slide.classList.toggle("is-active", offset + 1 === current);
    });

    if (controls) {
      controls.querySelectorAll("button").forEach((button, offset) => {
        button.setAttribute("aria-current", String(offset + 1 === current));
      });
    }

    if (updateUrl && !window.location.search) {
      window.history.replaceState(null, "", "?slide=" + current);
    }
  }

  function move(delta) {
    show(current + delta, true);
  }

  if (controls) {
    slides.forEach((_, offset) => {
      const button = document.createElement("button");
      button.type = "button";
      button.title = "Slide " + (offset + 1);
      button.setAttribute("aria-label", "Go to slide " + (offset + 1));
      button.addEventListener("click", () => show(offset + 1, true));
      controls.appendChild(button);
    });
  }

  document.addEventListener("keydown", (event) => {
    if (["ArrowRight", "PageDown", " "].includes(event.key)) {
      event.preventDefault();
      move(1);
    } else if (["ArrowLeft", "PageUp"].includes(event.key)) {
      event.preventDefault();
      move(-1);
    } else if (event.key === "Home") {
      event.preventDefault();
      show(1, true);
    } else if (event.key === "End") {
      event.preventDefault();
      show(slides.length, true);
    }
  });

  show(current, false);
})();
