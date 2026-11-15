# ADR 0017 — Aggiornamenti: meccanismo, firma, trasporto, versioni

- **Stato**: accettata con il sotto-passo P0.5e per la variante `dev`
- **Data**: 15 novembre 2026
- **Origine**: modifica 5 dell'utente al documento 0 (aggiornamento dal PC di sviluppo); decisioni vincolanti sullo schema A/B

## Contesto

Il 285K va aggiornato dal PC di sviluppo senza chiavetta, e ogni aggiornamento deve esercitare lo schema A/B. Il registro delle modifiche aveva fissato lo strumento (`systemd-sysupdate`) e il principio (impronte firmate, trasporto non fidato), lasciando da verificare come il 285K raggiunge un server che gira dentro WSL2 e come si numerano le costruzioni di sviluppo.

## Decisione

1. **Meccanismo**: `systemd-sysupdate`, con tre trasferimenti applicati insieme: albero di hash, radice, immagine di avvio. Le partizioni di destinazione sono quelle libere, riconosciute dall'etichetta `_empty`; l'immagine di avvio arriva con tre tentativi.
2. **Fiducia**: il server non è fidato. Ogni rilascio porta l'elenco delle impronte (`SHA256SUMS`) e la sua firma (`SHA256SUMS.gpg`); la macchina accetta solo firme della chiave pubblica contenuta nell'immagine.
3. **Chiave di firma degli aggiornamenti**, variante `dev`: una chiave OpenPGP creata alla prima costruzione in `keys/update/`, fuori dal repository, non cifrata. È distinta dalla chiave di Secure Boot.
4. **Trasporto**: un tunnel SSH inverso. Il PC di sviluppo serve la cartella del rilascio sulla propria interfaccia locale; la connessione SSH verso la macchina porta un tunnel all'indietro, e la macchina scarica da `127.0.0.1`. È il comando `make remote-update`.
5. **Nessun indirizzo di server nell'immagine**: lo strumento `dawos-update` lo riceve come argomento. Le definizioni dei trasferimenti sono modelli, completati al momento dell'uso.
6. **Versione delle costruzioni `dev`**: numero di rilascio seguito da data e ora UTC della costruzione, per esempio `0.0.1.20261115101500`. Le immagini di rilascio portano il numero nudo.
7. **Due immagini al massimo** sul disco: quella in uso e un'altra.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Server HTTP raggiungibile dalla rete (`make serve`), con WSL2 in modalità di rete speculare e una regola nel firewall di Windows | Era il piano del registro delle modifiche, segnato «da verificare». Richiede di cambiare la configurazione di rete di WSL2 e il firewall del PC di sviluppo, ed espone un servizio sulla rete locale. Il tunnel non richiede nulla di tutto questo. |
| Copiare i file con `scp` e scriverli con `dd` | Salterebbe proprio il meccanismo che deve essere collaudato a ogni iterazione, e la verifica della firma. |
| Fidarsi del trasporto (HTTPS, o lo stesso SSH) invece che di una firma | Lega la sicurezza dell'aggiornamento a quella del canale e del server. Con la firma, lo stesso rilascio resta verificabile da qualunque fonte arrivi: è ciò che servirà per gli aggiornamenti dei rilasci veri. |
| Indirizzo del server scritto nell'immagine da `local/` | Con il tunnel l'indirizzo è sempre lo stesso e non dice nulla; negli altri casi cambierebbe da rete a rete. |
| Usare la chiave di Secure Boot anche per gli aggiornamenti | Sono due ruoli diversi (che cosa il firmware avvia, che cosa la macchina accetta di installare) con formati diversi. Tenerle separate permette di sostituirne una senza toccare l'altra. |
| Contatore di costruzione al posto di data e ora | Va conservato da qualche parte e può tornare indietro cancellando la cartella di costruzione. L'ora non richiede stato. |
| Il segno `+` tra numero di rilascio e data | Nei nomi dei file il `+` è già il separatore del contatore dei tentativi di avvio: ambiguità da evitare. |

## Conseguenze

- Un aggiornamento è un comando sul PC di sviluppo; non serve la chiavetta né alcuna configurazione di rete.
- Un rilascio alterato sul server, anche con le impronte ricalcolate, viene rifiutato: chi controlla il server non possiede la chiave. Parte dei dati può essere stata scritta nello slot libero prima del rifiuto; lo slot resta marcato come vuoto e nulla diventa avviabile.
- Ogni `make image` produce una versione nuova: due costruzioni dello stesso sorgente hanno etichette diverse.
- Le etichette delle partizioni contengono la versione e hanno un limite di 36 caratteri: con questo schema `dawos-verity-<versione>` arriva a 33–36. `post-image.sh` lo controlla.
- Perdere `keys/update/` impedisce di aggiornare le macchine già installate: vanno reinstallate con un'immagine che contenga la chiave nuova.
- L'immagine contiene GnuPG, che systemd usa per verificare la firma, e gli strumenti di scaricamento di systemd.
- Lo stato in `/var` è condiviso tra le due immagini (ADR 0016).

## Quando si riapre

- **Aggiornamenti dei rilasci** (`prod`): server vero, chiave custodita come quella di Secure Boot (decisione D-23), eventuale aggiornamento automatico. Fase 6.
- **Compressione** delle immagini sul server, se le dimensioni lo richiederanno: oggi la radice è di circa 50 MiB e viaggia su una rete locale.
- **Ritorno a una versione precedente a comando**: oggi il ritorno è solo automatico, per un'immagine che non parte. `systemd-sysupdate` non installa una versione più vecchia di quella presente.
