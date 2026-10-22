// The ribbon's three strips, as ribbon.html lays them but at the OG
// picture's scale (og.css): the same books and themes, so the preview is
// recognisably the home.
const rows = [
  { top: 70,   shift: 0,    pairs: [["tape","tokyo-night"],["macro","gruvbox"],["desk","nord"],["crypto","everforest"],["swing","catppuccin"],["tape","kanagawa"],["desk","ristretto"]] },
  { top: 630,  shift: -400, pairs: [["desk","catppuccin"],["crypto","matte-black"],["swing","tokyo-night"],["tape","nord"],["macro","osaka-jade"],["desk","gruvbox"],["crypto","lumon"],["tape","everforest"]] },
  { top: 1190, shift: -125, pairs: [["macro","everforest"],["tape","ristretto"],["desk","kanagawa"],["swing","nord"],["crypto","tokyo-night"],["macro","catppuccin-latte"],["desk","ethereal"]] },
];
const band = document.getElementById("band");
rows.forEach(({ top, shift, pairs }) => {
  const row = document.createElement("div");
  row.className = "row"; row.style.top = top + "px"; row.style.left = shift + "px";
  pairs.forEach(([book, theme]) => {
    const img = new Image(); img.className = "tile";
    img.src = `captures/${book}-${theme}.png`; row.appendChild(img);
  });
  band.appendChild(row);
});
