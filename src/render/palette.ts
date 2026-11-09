export type Theme = 'light' | 'dark';

type Colour = [number, number, number];

export interface Palette {
  paper: Colour;
  line: Colour;
  ink: Colour;
  trail: Colour;
  ghost: Colour;
  /** Added towards the donut's silhouette: darkens it on paper, lights it on the night sky. */
  rim: number;
}

/** Paper and ink have to match `--paper` and `--ink` in styles.css. */
export const PALETTES: Record<Theme, Palette> = {
  light: {
    paper: [1, 1, 1],
    line: [0.835, 0.878, 0.925],
    ink: [0.098, 0.149, 0.212],
    trail: [0.49, 0.706, 0.941],
    ghost: [0.31, 0.576, 0.863],
    rim: -0.1,
  },
  dark: {
    paper: [0.063, 0.086, 0.122],
    line: [0.165, 0.208, 0.267],
    ink: [0.894, 0.922, 0.953],
    trail: [0.31, 0.576, 0.863],
    ghost: [0.49, 0.706, 0.941],
    rim: 0.14,
  },
};
