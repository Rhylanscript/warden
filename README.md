# Warden

A compiled language where every function declares what its allowed to touch.

## Idea

```warden
    fn load(path: str) -> Config uses fs.read { ... }
    fn parse(text: str) -> Config { ... }   // pure, provably
```

Effects (`fs`, `net`, `env`, `time`, `rand`) are checked at compile time.
