# ADR 0015 — Secure Boot: che cosa si firma, con quale chiave, con quali strumenti

- **Stato**: accettata con il sotto-passo P0.5c per la variante `dev`; apre le decisioni D-23 e D-24
- **Data**: 15 novembre 2026
- **Origine**: passo P0.5 del documento 0, decisioni vincolanti sulla catena di avvio

## Contesto

Le decisioni vincolanti fissano la forma della catena di avvio: immagine di avvio unica firmata, systemd-boot, radice EROFS con dm-verity. Non fissano con quale chiave si firma, che cosa si iscrive nel firmware delle macchine, con quali strumenti si costruisce, né fin dove arrivano le protezioni del kernel in funzione.

## Decisione

1. **Si firmano il boot loader e le immagini di avvio**, con un solo certificato autofirmato (RSA 2048, SHA-256: ciò che ogni firmware UEFI accetta).
2. **La chiave della variante `dev` è una chiave di sviluppo**, creata alla prima costruzione in `keys/secure-boot/`, fuori dal repository, non cifrata.
3. **Sulle macchine di sviluppo il certificato si aggiunge all'archivio delle firme (`db`) del firmware**, lasciando le chiavi esistenti del produttore e di Microsoft.
4. **Strumenti**: `ukify` dal pacchetto `host-systemd` di Buildroot; `sbsign` e `sbverify` dal pacchetto `sbsigntool` del PC di sviluppo; `virt-fw-vars` (pacchetto `python3-virt-firmware`) per iscrivere il certificato nel firmware di QEMU.
5. **Le varianti della riga di comando sono profili** della stessa immagine firmata.
6. **Blocco del kernel e vincolo sui moduli restano disattivati** nella variante `dev`.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Sostituire sulle macchine tutta la gerarchia di chiavi con la nostra (chiave di piattaforma compresa) | È la configurazione più forte, ma sul 285K toglie la possibilità di avviare qualunque altra cosa firmata, e un errore si ripara solo azzerando le impostazioni del firmware. Per una macchina di sviluppo il rischio non vale il guadagno. Resta l'opzione naturale per le macchine di rilascio. |
| Passare da _shim_, firmato da Microsoft | Serve a chi distribuisce a macchine di cui non controlla il firmware. Richiede una revisione esterna a ogni versione. Fuori scala per ora. |
| Far costruire `sbsign` a Buildroot | Buildroot non lo offre; il pacchetto `host-systemd` disattiva OpenSSL e quindi non costruisce lo strumento di firma di systemd. Cambiarne la configurazione dall'albero esterno funziona ma è fragile. |
| Chiave di sviluppo cifrata con una passphrase | Ogni `make image` chiederebbe la passphrase. Per una chiave che protegge solo macchine di sviluppo è attrito senza beneficio. |
| Voci di avvio tradizionali con kernel e riga di comando separati | La riga di comando resterebbe fuori dalla firma: è esattamente ciò che questo passo deve eliminare. |

## Conseguenze

- **Che cosa garantisce sul 285K**: con Secure Boot attivo, il boot loader e l'immagine di avvio di dawOS non si possono alterare senza che il firmware li rifiuti, e attraverso l'impronta radice nemmeno la radice.
- **Che cosa non garantisce**: il 285K avvia anche qualunque cosa sia firmata dalle chiavi già presenti nel firmware (per esempio un supporto d'installazione di Windows o una distribuzione con _shim_). Chi ha accesso fisico e al menu del firmware può spegnere Secure Boot. La protezione vale contro la modifica di dawOS, non contro l'avvio di altro.
- Con Secure Boot attivo la riga di comando non si modifica dal menu di avvio. Per provare un parametro si spegne Secure Boot, oppure si aggiunge un profilo.
- Perdere `keys/secure-boot/` obbliga a iscrivere un certificato nuovo su ogni macchina.
- Il PC di sviluppo ha due pacchetti in più, controllati da `make check-env`.

## Decisioni che apre

### D-23 — Chiave delle immagini di rilascio

Dove vive e come si usa la chiave che firmerà le immagini `prod`: su un dispositivo hardware dedicato, in un servizio di firma, o fuori linea. E se le macchine di rilascio useranno la nostra gerarchia completa di chiavi. **Proposta: rinviare alla Fase 6**, insieme allo sblocco dei dischi con il TPM, che dipende dalle stesse scelte. Va tra i punti per il parere legale e di sicurezza.

### D-24 — Protezioni del kernel in funzione

| Protezione | Che cosa fa | Costo |
| --- | --- | --- |
| Blocco del kernel, livello «integrità» (`lockdown=integrity`) | Impedisce anche all'amministratore di scrivere nella memoria del kernel, caricare moduli non firmati, scrivere nei registri del processore | Blocca l'accesso a `debugfs` e la scrittura dei registri: servono agli esperimenti E-01 ed E-02 del passo P0.6 e a parte degli strumenti di misura |
| Vincolo sui moduli (LoadPin) | Il kernel carica moduli e firmware solo dalla radice verificata | Nessuno, se tutti i moduli stanno nella radice |

**Proposta**: nella variante `dev` nessuna delle due, per non ostacolare le misure della Fase 0; nella variante `prod` entrambe. LoadPin si può anticipare senza costi: da decidere prima del passo P0.8.

## Quando si riapre

- Alla definizione della variante `prod` (D-23, D-24).
- Se una macchina di destinazione avesse un firmware che non permette di aggiungere certificati all'archivio delle firme.
- Se si decidesse di distribuire dawOS a macchine di terzi: tornerebbe in gioco _shim_.
