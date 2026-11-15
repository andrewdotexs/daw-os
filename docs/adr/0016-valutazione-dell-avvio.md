# ADR 0016 — Valutazione dell'avvio e ritorno all'immagine precedente

- **Stato**: accettata con il sotto-passo P0.5d
- **Data**: 15 novembre 2026
- **Origine**: passo P0.5 del documento 0 («prova di ritorno»); decisioni vincolanti sullo schema A/B

## Contesto

Le decisioni vincolanti prevedono due slot per l'immagine di sistema e il ritorno automatico a quello precedente. Il documento 0 indica il meccanismo (contatore dei tentativi di systemd-boot, conferma con `systemd-bless-boot`) ma non dice che cosa conta come avvio riuscito, né che cosa succede quando un avvio non riesce senza che la macchina si riavvii da sé.

## Decisione

1. **Meccanismo**: la valutazione dell'avvio di systemd. Il contatore dei tentativi sta nel nome del file dell'immagine di avvio; una nuova immagine viene installata come `dawos_<versione>+3.efi`.
2. **Tre tentativi** per ogni immagine nuova, compresa quella di una prima installazione.
3. **Criterio di riuscita, per ora**: nessuna unità di systemd in stato di errore al termine dell'avvio (`systemd-boot-check-no-failures.service`). I controlli specifici di dawOS si aggiungeranno come ulteriori requisiti di `boot-complete.target`.
4. **Un avvio in prova che non passa i controlli riavvia la macchina** (`dawos-boot-failed`), fino al terzo tentativo compreso. Oltre, e per le immagini già confermate, la macchina resta accesa.
5. **Nessun indicatore di slot attivo**: ogni immagine di avvio nomina per etichetta le proprie partizioni, e l'etichetta contiene la versione.
6. **La variante `dev` contiene un'unità che fa fallire l'avvio a comando** (`dawos-fail-boot-test`), per provare il ritorno sulle macchine reali.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Indicatore dello slot attivo in una variabile del firmware o in un file, gestito da noi | È ciò che fanno altri sistemi A/B, ma aggiunge uno stato da tenere allineato con il contenuto dei due slot. Qui lo stato è il nome di un file, e lo aggiorna il boot loader. |
| Lasciare accesa la macchina quando un controllo fallisce | Un'immagine difettosa ma avviabile resterebbe in servizio finché qualcuno non interviene. In uno studio significa trovare la macchina guasta al mattino. |
| Riavviare sempre, anche esauriti i tentativi | Se non c'è un'altra immagine da avviare, sarebbe un ciclo senza fine e senza possibilità di diagnosi. |
| Un solo tentativo | Un errore passeggero (in emulazione ne ho visto uno, vedi la guida del sotto-passo) scarterebbe un'immagine buona. |
| Nessun contatore sulla prima installazione | La conferma non verrebbe mai esercitata fuori dagli aggiornamenti; con il contatore ogni installazione la prova. |

## Conseguenze

- Un aggiornamento difettoso costa al massimo tre avvii falliti, poi la macchina torna in servizio con l'immagine precedente, senza intervento.
- Un'unità che fallisce su un'immagine **in prova** provoca un riavvio: durante lo sviluppo, un'immagine con un servizio rotto non resta accesa per essere esaminata, se esiste un'immagine precedente. Per esaminarla basta guardare il registro dall'immagine precedente: `/var` è condivisa.
- **`/var` è condivisa tra i due slot.** Il ritorno riporta indietro il sistema, non i dati. Regola da rispettare da qui in poi: un'immagine nuova non deve rendere illeggibile, per quella precedente, ciò che scrive in `/var`; i cambi di formato vanno fatti in modo che l'immagine precedente continui a partire. Va ripresa quando si definiranno le impostazioni e il registro delle sessioni (Fase 2).
- La partizione di avvio deve essere scrivibile dal boot loader e dal sistema: è lì che vive il contatore.

## Quando si riapre

- **Watchdog hardware.** Un kernel che si blocca senza errore non consuma tentativi e non fa ripartire nulla. Il rimedio è il temporizzatore hardware, comandato da systemd. Va prima misurato sul 285K: su alcune schede lavora tramite le interruzioni di gestione del sistema (SMI), e il passo P0.4b ne ha contate zero. Da decidere prima del passo P0.8.
- **Criterio di riuscita.** Quando esisteranno il motore audio e il servizio di salute del sistema.
- **Numero di tentativi**, se tre si rivelassero troppi (tempo perso) o troppo pochi (immagini buone scartate).
