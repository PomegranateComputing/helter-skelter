declare function registerPlugin(plugin: {
  name: string;
  version: string;
  authors: string[];
  type: "local" | "remote" | "intransient";
  licence: string;
  main: () => void;
}): void;
