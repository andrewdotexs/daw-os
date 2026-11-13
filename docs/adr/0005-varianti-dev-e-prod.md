# ADR 0005 — Varianti `dev` e `prod` dell'immagine

- **Stato**: accettata
- **Data**: 13 novembre 2026
- **Origine**: D-05 e C-05 del documento 0

## Contesto

Il master prompt vieta servizi in ascolto e accessi come root in esercizio. Per sviluppare servono però una shell sul 285K, gli strumenti di misura e un modo per raggiungere il server MCP prima che esista l'agente.

## Decisione

Due varianti costruite dallo stesso albero `BR2_EXTERNAL`:

| Aspetto | `dev` | `prod` |
| --- | --- | --- |
| Accesso remoto | SSH con sola chiave | nessuno |
| Console | seriale e schermo, accesso locale come root | nessuna shell |
| Strumenti | `rtla`, `cyclictest`, `turbostat`, `perf`, `trace-cmd`, `stress-ng`, `fio` | nessuno |
| MCP | socket Unix locale, raggiungibile con tunnel SSH | solo socket Unix locale |
| Firma | chiave di sviluppo | chiavi di produzione |
| Aggiornamenti | server HTTP sul PC di sviluppo | release GitHub |
| Rapporto di avvio sulla console | sì | no |

Nella Fase 0 esiste solo `dev`. La variante compare in `VARIANT_ID` di `os-release` e come secondo argomento degli script di costruzione.

## Alternative scartate

- **Una sola immagine con una «modalità sviluppatore» attivabile**: un interruttore a runtime è superficie d'attacco anche in produzione.

## Conseguenze

- Le regole di sicurezza del master prompt valgono per `prod`. Ogni deroga di `dev` è elencata nella tabella sopra e in nessun altro posto.
- Le chiavi pubbliche SSH autorizzate non stanno nel repository: si prendono da un file locale ignorato da git al momento della costruzione (passo P0.4).
- Nel passo P0.2 la variante `dev` ammette l'accesso come root senza password dalla sola console. Non ci sono servizi raggiungibili dalla rete _(verificato in QEMU: restano solo il risolutore locale su `127.0.0.53` e il client DHCP)_, quindi il rischio è limitato a chi ha accesso fisico; dal passo P0.4 la password viene bloccata e resta solo la chiave SSH.
- Un'immagine `dev` non deve mai essere firmata con chiavi di produzione: lo impone lo script di firma, non la disciplina.

## Aggiornamento del 13 novembre 2026

Con il passo P0.4a, due precisazioni approvate:

- **La password di root resta vuota per la sola console**, anche dopo il passo P0.4. La frase qui sopra che ne annunciava il blocco è superata: al primo avvio sul 285K la console è l'unico modo per diagnosticare una rete che non sale. Via SSH le password sono disattivate, quindi una password vuota non apre nulla.
- **La chiave dell'host SSH delle immagini `dev`** viene generata sul PC di sviluppo, nella cartella `keys/` ignorata da git, e inclusa nell'immagine, perché la radice è in sola lettura. In `prod` la chiave nascerà sulla macchina, sulla partizione `state`.

## Quando si riapre

Nella Fase 6, quando nasce `prod` e si definisce cosa resta di diagnostico in esercizio.
