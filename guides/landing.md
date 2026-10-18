# Landing and marketing pages

A landing page has one job: within the first screen a visitor sees (about 1440×900 on a desktop,
nav included), say what the product is and show it. Every section after that proves one claim.
The pages people admire (Linear, Stripe, Vercel, incident.io) feel expensive mostly through
restraint and finish, not through more ideas: few elements, one voice, real material, and every
detail consistent. Most generated pages miss on exactly that, so plan for it and leave time for a
refinement pass at the end.

## Structure

- **Plan the story.** Add a section outline to your plan: the headline claim, the hero visual,
  three to five claims each with its own evidence, proof, the closing ask. Write the real headline
  and section titles before drawing.
- **The first screen.** A short headline (two lines at most) set large, a sentence or two of
  supporting text under ~60 characters per line, one primary and one quieter secondary action. The
  hero visual starts inside the first screen.
- **Show the product, big.** The hero visual is the most important element on the page: a
  believable product screen at screenshot fidelity (app chrome, navigation, many rows of real
  content, one highlighted moment that tells the story), sized to dominate, often the full content
  width and cropped by the bottom of the screen or the edge of the page. When the product has no
  screen worth showing, one bold visual instead: a photograph, a rendered object, a large graphic.
- **Every feature shows its evidence.** Pair each claim with the specific UI that proves it (the
  notification, the chart, the diff, the setting), and vary the composition between sections: text
  beside a visual, a full-width visual under a short heading, a statement set large on its own, a
  grid of fragments of different sizes. A row of identical icon-title-paragraph cards is the
  template default; keep it for minor detail lists.
- **Rhythm.** One content width and shared left edges, generous space between sections, and
  scale contrast between them: a huge statement next to a dense, small-print grid reads as designed;
  every section at the same heading size, width and padding reads as generated.

## Restraint: signs a page was generated

Check your page against these. Each is fine as one deliberate choice; several together are what
makes a page look machine-made.

- **Headline tricks.** A second line or a single word recolored in the accent, or switched to
  italic, to make it "pop". Refined pages set the headline in one color and get emphasis from
  scale and weight, or let a longer sentence continue in a quieter tone of the same color.
- **Formula copy.** Every heading a two-beat antithesis or punchline ("One command. Sixty-four
  GPUs.", "Not X. Y.", "Snap it, text it, done."), triplets everywhere, a joke in every subhead.
  Write most headings as plain statements of what the product does; save the one clever line for
  where it lands hardest.
- **Labels on everything.** A numbered mono eyebrow ("01 · SPEND CONTROLS") over every section, a
  "New" chip, a badge on each card, checkmark bullet lists under each paragraph. Use a labelling
  system only if it means something, and then use it sparingly.
- **The floating toast.** A notification card overlapping the corner of every mockup. Once, where
  it tells the story, is good; on every visual it's a tic.
- **Boxes in boxes.** A tinted rounded panel holding a card holding another card, repeated per
  section. Let some visuals sit directly on the page, bleed off an edge, or fill a full-width band.
- **Fake proof.** Made-up customer names each set in a different font, initials in pastel circles
  instead of faces, a row of three big round-number stats under every quote. Draw logo-like marks
  that share one weight and one color at an even optical size (a simple mark from an icon set plus
  a wordmark works), use a real photo (`search_images`) or no avatar, and show figures only where
  they're the point.
- **Flat material.** Every surface a flat fill with the same 1px grey border and the same radius,
  no light anywhere. Give the hero and one or two key visuals real depth: a soft gradient or
  light falling on the product, a photograph, layered surfaces with a subtle shadow, a rendered
  object. Keep the rest quiet so those moments carry.
- **Inconsistent details.** Mixed corner radii between buttons, inputs and cards, two styles of
  primary button, icons at different stroke weights, monospace used for random numbers. Pick each
  once and reuse it.
- **A house style.** Pastel ground, serif headline with an italic accent word, a quote beside a
  stats column, a rounded tinted panel for the closing call: fine once, a template when it's every
  page. Choose the ground (light, dark, photographic, saturated), type voice and closing layout
  from the brief, and find the page's one memorable device in the product's own world. If the page
  could swap its logo with another startup's and still make sense, it isn't specific enough.

## Refinement pass

When every section exists, stop adding. Screenshot the hero on its own and the whole artboard,
and go through it as an editor:

1. Name the single most striking thing on the page. If nothing stands out, that's the next fix;
   if three things compete, quiet two of them. Restraint isn't blandness: a page made only of flat
   panels on a plain ground needs one moment of real material (light on the product, a
   photograph, a rendered object, a saturated band) before it's done.
2. Walk the list above and remove what it flags: recolored headline words, labels, toasts, extra
   chips, bullet lists, stats rows. Removing elements usually raises quality more than adding.
3. Unify details: one radius scale, one button style per role, one border color, one icon set and
   size, consistent space between sections.
4. Reread every heading out loud. Rewrite the ones that sound like ad copy into plain statements.
5. Compare neighbouring sections: same layout, same scale, same ground twice in a row means one
   of them should change.

Everything in the main `cardboard-design` guide still applies: build in small steps, look at
each section with `screenshot`, and use components for repeated rows.
