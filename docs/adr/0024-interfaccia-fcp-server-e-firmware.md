# ADR 0024 — L'interfaccia di quarta generazione: `fcp-support` e firmware nell'immagine

- **Stato**: accettata dall'utente il 24 novembre 2026 nelle tre scelte che gli sono state poste (firmware nell'immagine, aggiornamento prima delle misure, nome del passo); le altre scelte sono mie, dichiarate nel [passo P1.12](../fasi/fase-1/P1.12-la-scarlett-di-quarta-generazione.md) e non contestate. Da confermare con le misure sul 285K
- **Data**: 24 novembre 2026
- **Origine**: passo [P1.12](../fasi/fase-1/P1.12-la-scarlett-di-quarta-generazione.md); domanda D-28; prompt della Fase 1, che nomina `fcp-server`

## Contesto

La Scarlett 18i20 di quarta generazione (USB `1235:821d`) è arrivata il 24 novembre 2026. Suona e registra con il driver USB audio del kernel come ogni interfaccia conforme alla classe, ma il suo router, il suo mixer e i suoi preamplificatori non si vedono: il driver che il kernel ha per lei (`sound/usb/fcp.c`, in Linux dalla 6.14; nel nostro 6.18.64 _(verificato nel sorgente e nel registro della macchina: «Focusrite Control Protocol Driver ready»)_) porta soltanto messaggi avanti e indietro. Che cosa significano lo sa un programma in spazio utente, `fcp-server`, del pacchetto [fcp-support](https://github.com/geoffreybennett/fcp-support) di Geoffrey D. Bennett, lo stesso autore del driver.

Per la seconda generazione era diverso: il driver `scarlett2` del kernel conosce il protocollo e crea da solo i controlli del mixer.

Senza `fcp-server` il motore non può sapere che cosa portano i canali che apre: il routing è quello che l'interfaccia ha in memoria, e non si legge. Il passo P1.4 ha stabilito che il routing si porta a uno stato noto prima di ogni misura.

Il firmware dell'interfaccia è un file di Focusrite, pubblicato per Linux nel repository [scarlett4-firmware](https://github.com/geoffreybennett/scarlett4-firmware). Non è software libero: la licenza (`LICENSE.Focusrite`) permette di ridistribuirlo senza modifiche e solo per l'uso con prodotti Focusrite, e vieta di analizzarlo _(verificato: letta)_.

## Decisione

**L'immagine contiene `fcp-support` e il file di firmware della 18i20; il firmware si scrive nell'interfaccia solo con un comando dato a mano.**

1. **`fcp-support` è un pacchetto di Buildroot nostro** (`external/package/fcp-support`), a un commit fissato con hash: `02be3c9` del 5 novembre 2026, quattro commit dopo l'etichetta `1.0beta1`.
2. **L'unità e la regola di udev sono quelle del pacchetto, senza modifiche.** udev avvia `fcp-server@N.service` per ogni scheda audio di Focusrite; il servizio gira con un utente creato al momento, il gruppo `audio` e la sola capacità `CAP_SYS_RAWIO`, quella che il driver chiede per aprire il canale dei messaggi.
3. **`fcp-server` resta acceso finché l'interfaccia è collegata**, anche durante l'audio. Gira dove girano i servizi: su tutti i core tranne quelli live, che sono isolati all'avvio _(verificato in macchina virtuale con il profilo `285k`: core permessi 0-3 e 8-23)_. Tra quelli ci sono i core del dominio `io`, dove lavora il thread dell'interrupt USB.
4. **Il firmware è un secondo pacchetto** (`external/package/scarlett4-firmware`), al commit dell'etichetta `1.1` (firmware 2778). Installa il solo file della 18i20. Nel repository di dawOS il file non entra.
5. **Nulla aggiorna il firmware da sé.** Lo fa `tools/card-firmware.sh update`, a mano, che rifiuta se la scheda è in uso.
6. **L'interfaccia si aggiorna alla versione dell'immagine prima di misurarla.** Ogni misura riporta così un firmware noto.
7. **Lo stato del routing si legge e si scrive con `dawos-card-setup`**, come per la seconda generazione: i controlli che `fcp-server` crea sono controlli ALSA come gli altri.

## Perché

- **Non c'è un'altra via per il routing.** Il protocollo non è documentato da Focusrite; `fcp-server` è l'unico programma che lo parla, e lo mantiene chi ha scritto il driver.
- **Un firmware noto vale più di un'immagine tutta libera**, in una fase in cui ogni numero misurato deve poter essere ripetuto. `fcp-server` descrive i controlli dell'interfaccia con una mappa scritta per il firmware che il suo autore ha provato: con un altro firmware un controllo può mancare o essere letto male _(ipotesi; il sorgente salta i campi che non trova)_.
- **Il commit e non l'etichetta `1.0beta1`**: tra i quattro commit successivi c'è una correzione al controllo `Sync Status`, che restava su «agganciato» dopo un cambio di clock. Gli altri tre riguardano un'altra interfaccia e l'impacchettamento.
- **L'unità del pacchetto e non una nostra**: è quella che gli altri usano e su cui arrivano le correzioni. Chiede meno privilegi di quanti ne darei io scrivendola in fretta.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Nessun `fcp-server`: l'interfaccia con il routing che ha | Non si sa che cosa portano i canali, e non si può fare il giro interno che la verifica del conteggio usa. |
| Portare l'interfaccia a uno stato noto una volta, da un altro sistema, con il programma di Focusrite | Lo stato non sarebbe verificabile da dawOS prima di una misura. E il prodotto dovrà comandare i preamplificatori dall'interfaccia della DAW. |
| Riscrivere in Rust la parte del protocollo che serve | È il lavoro di mesi di un altro, senza documentazione del protocollo. Si può riprendere se `fcp-server` risulta inadatto a stare acceso durante l'audio. |
| `fcp-server` avviato solo quando serve, spento durante l'audio | Il prodotto avrà i controlli dei preamplificatori sempre in vista. Meglio misurare subito la configurazione vera: lo dicono le due ore del passo P1.12c. |
| Il firmware copiato a mano sulla macchina | L'immagine sarebbe tutta software libero, ma la versione del firmware non sarebbe legata alla build. |
| Il firmware aggiornato da Windows o macOS con Focusrite Control 2 | Non sapremmo se quella versione è quella provata con Linux. |
| L'etichetta `1.0beta1` | Manca la correzione a `Sync Status`. Si passa a una versione etichettata quando esce la prossima. |

## Conseguenze

- **L'immagine contiene un file non libero**, di 1,1 MB, con la sua licenza. Quando dawOS sarà distribuito, la licenza di Focusrite va riprodotta nella documentazione che accompagna l'immagine: Buildroot la raccoglie con `make legal-info`. Da rivedere prima di ogni distribuzione.
- **Un programma in più parla con l'interfaccia mentre suona.** `fcp-server` dorme finché un controllo cambia o l'interfaccia lo avvisa (una manopola girata, un cambio di clock), poi fa trasferimenti di controllo sullo stesso bus USB dell'audio _(verificato nel sorgente: il ciclo principale aspetta senza scadenza)_. Quanto disturbi non è misurato.
- **Due modifiche nostre al `Makefile` del pacchetto**, come patch: il collegamento con il compilatore della build invece di `cc`, e niente `-Werror`. La prima è un difetto da segnalare all'autore.
- **Con la seconda generazione collegata `fcp-server` parte e finisce subito, senza errore** _(verificato nel sorgente: riconosce il driver `scarlett2` dalla versione del protocollo ed esce con stato 0; da verificare sulla macchina alla prima occasione)_.
- **I numeri di serie.** `fcp-tool` stampa il numero di serie dell'interfaccia. Gli script che lo usano lo tolgono dalla loro uscita; chi lo lancia a mano deve ricordarsene prima di incollare l'uscita in un file del repository.
- **L'aggiornamento del firmware non è reversibile con ciò che abbiamo** _(da verificare: le versioni precedenti sono nel repository del firmware, ma non so se l'interfaccia le accetta)_.

## Quando si riapre

- Se le due ore del passo P1.12c mostrano che `fcp-server` acceso disturba l'audio.
- Se una versione nuova del firmware o di `fcp-support` esce: si cambia il commit, si ricostruisce, si rimisura la latenza.
- Quando dawOS si distribuisce: la licenza del firmware, e se l'aggiornamento va offerto all'utente dall'interfaccia della DAW.
- Se il prodotto deve sostenere altre interfacce di Focusrite: oggi si installa il solo firmware della 18i20.
