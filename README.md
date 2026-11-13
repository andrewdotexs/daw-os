# daw-os

dawOS: un sistema operativo basato su Linux in cui la DAW è l'unico programma.

Stato: **Fase 0 — Piattaforma**, passi P0.1 e P0.2 consegnati.

## Per cominciare

```bash
git clone --recurse-submodules https://github.com/andrewdotexs/daw-os.git
```

```bash
make check-env
```

```bash
make image
```

```bash
make qemu-test
```

`make help` elenca tutti i bersagli. I dettagli di ogni passo sono in `docs/fasi/`.

## Documentazione

- [Documento 0: revisione critica, interfacce trasversali, piano della Fase 0, domande aperte](docs/00-revisione-specifiche-fase0.md)
- [Registro delle modifiche del documento 0 verso la versione 1](docs/01-registro-modifiche-v1.md)
- [Registri delle decisioni (ADR)](docs/adr/README.md)
- Fase 0: [P0.1 — Ambiente di sviluppo](docs/fasi/fase-0/P0.1-ambiente.md), [P0.2 — Scheletro e primo avvio in QEMU](docs/fasi/fase-0/P0.2-scheletro-e-qemu.md), [hardware di riferimento](docs/fasi/fase-0/hardware-285k.md)

## Licenza

GPLv3 con un permesso supplementare per i plugin (_dawOS Plugin Exception_): vedi [LICENSE](LICENSE) e l'[ADR 0010](docs/adr/0010-licenza-gplv3-eccezione-plugin.md).
