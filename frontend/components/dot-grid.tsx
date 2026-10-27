"use client";

import { useEffect, useRef } from "react";

const GAP = 22; // px between dots
const RADIUS = 1; // a resting dot
const REACH = 170; // px around the pointer that lights up
const GROW = 2.2; // extra radius at the pointer

/**
 * A field of faint dots that brighten and swell around the pointer. Drawn on
 * a canvas sized to its parent; repaints only while the pointer moves or
 * eases away. Colours come from the theme (foreground for dots, the sidebar
 * accent for the glow). Still for people who prefer reduced motion.
 */
export function DotGrid({ className }: { className?: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const el = canvas.current;
    const host = el?.parentElement;
    const ctx = el?.getContext("2d");
    if (!el || !host || !ctx) return;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    let width = 0;
    let height = 0;
    let pointer: { x: number; y: number } | null = null;
    // 0..1: how strongly the glow shows, easing in and out.
    let glow = 0;
    let frame = 0;
    let dot = "rgba(255,255,255,0.14)";
    let lit = "rgba(120,140,255,1)";

    function colours() {
      const style = getComputedStyle(el!);
      // Resolve the theme's colours through a probe, whatever their notation.
      const probe = (value: string) => {
        ctx!.fillStyle = "#000";
        ctx!.fillStyle = value;
        return ctx!.fillStyle as string;
      };
      dot = probe(style.getPropertyValue("--dot-color") || "rgba(255,255,255,0.14)");
      lit = probe(style.getPropertyValue("--dot-lit") || "#7c8cff");
    }

    function resize() {
      const rect = host!.getBoundingClientRect();
      const ratio = window.devicePixelRatio || 1;
      width = rect.width;
      height = rect.height;
      el!.width = Math.round(width * ratio);
      el!.height = Math.round(height * ratio);
      el!.style.width = `${width}px`;
      el!.style.height = `${height}px`;
      ctx!.setTransform(ratio, 0, 0, ratio, 0, 0);
      colours();
      draw();
    }

    function draw() {
      ctx!.clearRect(0, 0, width, height);
      const offsetX = (width % GAP) / 2 + GAP / 2;
      const offsetY = (height % GAP) / 2 + GAP / 2;
      for (let y = offsetY; y < height; y += GAP) {
        for (let x = offsetX; x < width; x += GAP) {
          let near = 0;
          if (pointer && glow > 0) {
            const d = Math.hypot(x - pointer.x, y - pointer.y);
            if (d < REACH) near = (1 - d / REACH) ** 2 * glow;
          }
          ctx!.beginPath();
          ctx!.arc(x, y, RADIUS + GROW * near, 0, Math.PI * 2);
          if (near > 0.01) {
            ctx!.globalAlpha = 0.35 + 0.65 * near;
            ctx!.fillStyle = lit;
          } else {
            ctx!.globalAlpha = 1;
            ctx!.fillStyle = dot;
          }
          ctx!.fill();
        }
      }
      ctx!.globalAlpha = 1;
    }

    function animate(target: number) {
      cancelAnimationFrame(frame);
      const step = () => {
        glow += (target - glow) * 0.18;
        if (Math.abs(target - glow) < 0.01) glow = target;
        draw();
        if (glow !== target) frame = requestAnimationFrame(step);
      };
      frame = requestAnimationFrame(step);
    }

    function move(event: PointerEvent) {
      const rect = el!.getBoundingClientRect();
      pointer = { x: event.clientX - rect.left, y: event.clientY - rect.top };
      if (glow < 1) animate(1);
      else {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(draw);
      }
    }

    function leave() {
      animate(0);
    }

    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(host);
    // The theme can switch while the page is open.
    const themed = new MutationObserver(() => {
      colours();
      draw();
    });
    themed.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style"] });
    if (!still) {
      host.addEventListener("pointermove", move);
      host.addEventListener("pointerleave", leave);
    }
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      themed.disconnect();
      host.removeEventListener("pointermove", move);
      host.removeEventListener("pointerleave", leave);
    };
  }, []);

  return <canvas ref={canvas} aria-hidden className={className} />;
}
