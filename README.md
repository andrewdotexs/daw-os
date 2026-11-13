# daw-os

dawOS: un sistema operativo basato su Linux in cui la DAW è l'unico programma.

Stato: **Fase 0 — Piattaforma**, passi da P0.1 a P0.4a superati, P0.4b consegnato.

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
- Fase 0: [P0.1 — Ambiente di sviluppo](docs/fasi/fase-0/P0.1-ambiente.md), [P0.2 — Scheletro e primo avvio in QEMU](docs/fasi/fase-0/P0.2-scheletro-e-qemu.md), [P0.3 — Kernel real-time](docs/fasi/fase-0/P0.3-kernel-real-time.md), [P0.4a — Installazione sul 285K](docs/fasi/fase-0/P0.4a-installazione.md), [P0.4b — Rumore di fondo](docs/fasi/fase-0/P0.4b-rumore-di-fondo.md), [hardware di riferimento](docs/fasi/fase-0/hardware-285k.md)

## Passi chiusi

`main` contiene solo i passi il cui criterio di accettazione è stato verificato; ciascuno ha un tag (`p0.1`, `p0.2`, …). Il lavoro in corso sta sul ramo di sviluppo. I dettagli sono nell'[ADR 0012](docs/adr/0012-main-e-tag-a-ogni-passo.md).

```bash
git log --first-parent --oneline main
```

## Licenza

GPLv3 con un permesso supplementare per i plugin (_dawOS Plugin Exception_): vedi [LICENSE](LICENSE) e l'[ADR 0010](docs/adr/0010-licenza-gplv3-eccezione-plugin.md).
