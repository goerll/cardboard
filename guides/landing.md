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

## Give every section a visual idea

What separates a designed section from a template one is rarely polish; it's that the section has
an idea you could describe as a picture. "The day's tide drawn as one long curve across a dark band,
with the safe sailing windows lit along it" is an idea. "Heading on the left, table on the right" is a layout. A
page where the hero has an idea and every later section is text in rows reads as cheap, however
clean the rows are.

- **Pick a world first.** Before drawing, write in your plan one line naming the page's world and
  three or four motifs taken from the product itself (its materials, instruments, artifacts,
  places, the marks it leaves): for a ceramics studio, raw clay and glaze tests, the kiln's
  temperature log, fingerprints in the rim, the handwritten firing tag. Reuse those motifs across sections; that
  recurrence is what reads as personality.
- **Write a visual idea per section.** In the section outline, give each section one sentence
  saying what the visitor sees, not what the section says, and which medium carries it: a
  photograph or rendered object for physical things and people, drawn SVG/CSS for data, diagrams
  and product UI. If the sentence is "a heading and some text", find the picture of the claim:
  - a spec list becomes a dimensioned drawing of the object, with the figures as callouts on it;
  - a list of options (plans, profiles, flavours, regions) becomes the things themselves: labels,
    swatches, cards set at real scale, or one chart where each option is a line;
  - "integrates with everything" becomes a diagram of the actual flow, not a logo grid;
  - "fast" or "secure" becomes the moment it shows: the timeline, the blocked attempt, the log line;
  - the closing call shows the product one last time (the object on a counter, the card in hand,
    the app on a phone) instead of a flat coloured band with a form.
  Let at most one section be text alone, and make that one a statement set very large.
- **One dominant move per section.** The visual should be the largest thing in its section and
  the first thing the eye lands on, with the text supporting it. Don't spread effort over three
  small visuals where one large one would carry.
- **Compose in depth.** The visuals that look most designed are built from two or three layers,
  not one flat screen: a phone in front of the dashboard it controls, the physical card resting on
  the transactions screen, a photograph behind a floating panel of the app, a single row lifted out
  of the table at a larger size with a deeper shadow. Overlap them so they share edges, give the
  front layer more shadow and the back layer less contrast, and let the arrangement tell the claim
  (the phone shows the alert that the dashboard caused).
- **Zoom into the moment.** A whole screen shrunk to fit is the weakest way to show a feature.
  Crop to the part that proves the claim and set it larger than life, around 1.2 to 1.5 times its
  real size, cut by the section edge or a mask, with the rest of the product implied around it.
  Keep the full screen for the hero, and even there let it run off the bottom of the first screen.
- **One signature piece.** Give the page one crafted art piece that nobody else's page has, often
  in the hero or the first section after it: a rendered object built from gradients and highlights,
  a mesh of colour, a field of hundreds of small marks forming a shape, an isometric drawing of the
  system. It's what visitors remember; it should be made from the page's world and motifs.
- **Light and material.** Commit to a dominant colour and use the accent sharply and rarely;
  timid, evenly spread palettes look generated. Vary the ground between sections (light, dark,
  photo, saturated) so the page has rhythm instead of one long sheet. Keep one light source and
  spend it on the key visuals. Software pages need this most, because a screenshot on a plain
  ground is the flattest thing on the web. Techniques that render well here:
  - *Stage the product.* Set the screenshot on its own lit field instead of the page ground: a
    band or panel with a soft gradient in the dominant colour, or a large glow behind it (an
    absolutely placed shape with `filter: blur(80px)` or more, kept inside a clipped parent).
  - *Embed, don't frame.* Fade a screenshot or a long list into the ground with
    `mask-image: linear-gradient(to bottom, #000 60%, transparent)`, or let it run off the edge,
    so it reads as part of a larger surface rather than a card dropped on the page.
  - *One view in perspective.* Tilt the hero or one feature visual in 3D (`perspective` on the
    parent, `transform: rotateX(..) rotateY(..)` on the child) with a long soft shadow below it.
  - *Grain on colour.* Gradients and saturated bands look printed rather than digital with a
    noise overlay: an absolute `<svg>` holding `<filter><feTurbulence type="fractalNoise"
    baseFrequency="0.8"/><feColorMatrix type="saturate" values="0"/></filter>` and a full-size
    `<rect filter="url(#id)">`, at `mix-blend-mode: overlay` and low opacity.
  - *Draw what isn't UI.* When a claim is about scale, architecture or speed, draw it as crafted
    illustration in the page's line weight: an isometric line drawing, a field of thousands of
    dots, a radial or flow diagram. Keep these in one drawing style across the page.
  - *Photographs lead.* When a strong photograph or object exists for the product, it leads the
    hero; don't trade it for a smaller drawing. Use `search_images` before deciding there isn't one.
- **Choose from a few.** For the sections that feel weakest, list two or three different visual
  ideas before drawing one and pick the most specific to this product. The first idea is usually
  the one every page has.

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
   of them should change. At least one section should sit on a saturated colour field or a
   full-bleed photograph; a page of only light and dark neutrals reads timid however well it's
   composed.
6. Cover the text of each section with your hand (or look at its screenshot at thumbnail size).
   If what's left is boxes and lines, the section still needs its visual idea. Compare each
   section with the hero: if the hero is the only part that looks designed, the later sections
   aren't done.

Everything in the main `cardboard-design` guide still applies: build in small steps, look at
each section with `screenshot`, and use components for repeated rows.
