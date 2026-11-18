# Punti aperti

- **Che cos'è**: l'elenco unico delle cose rimaste in sospeso lungo i passi. Ogni punto dice da dove viene, che cosa serve per chiuderlo e in quale passo si chiude.
- **Chi lo aggiorna**: si aggiorna alla chiusura di ogni passo. Un punto chiuso passa nell'ultima tabella con la data e il modo; non si cancella.
- **Che cosa non contiene**: le domande di progetto (D-xx) e i rischi (K-xx) hanno già il loro posto nel [registro delle modifiche](01-registro-modifiche-v1.md); qui compaiono solo quando c'è un lavoro da fare.
- **Stato**: aggiornato il 16 novembre 2026, alla chiusura del sotto-passo P0.6b e all'avvio di P0.6c; lo stesso giorno aggiunti A-27, A-28 e A-29, chiusi A-01, A-04, A-27 e A-28; il 17 novembre chiusi A-02 e A-03, alla chiusura del sotto-passo P0.6c, e avviato P0.6d; lo stesso giorno aggiunti A-31 e A-32, dall'esperimento E-06, e A-33 e A-34, dall'esperimento E-05; aggiunti A-35, aperto in anticipo nel sotto-passo P0.6e, e A-36; chiusi A-34 e A-31; scelto e verificato il rimedio per A-33, alla chiusura del sotto-passo P0.6d; poi, con la parte 2 di P0.6e, spostati A-06 e A-07 prima della Fase 1 e aggiunto A-37; verificata la parte 2 sul 285K, chiusi A-05, A-09, A-10, A-12, A-13, A-14 e A-15, aggiunti A-38 e A-39; alla chiusura del sotto-passo P0.6e chiusi A-08 e A-11, aggiunto A-40, e la configurazione è ferma; il 18 novembre, all'avvio della campagna P0.7, guardati i dati grezzi di A-40 e aggiunti A-41 e A-42; dopo il primo lancio, lo stesso giorno, aggiornati A-30 e A-33.

## La regola d'ordine

La campagna di misura P0.7 dura 24 ore e certifica **una** configurazione. Tutto ciò che può ancora cambiare il kernel, il firmware, la regolazione o gli strumenti di misura si chiude **prima**: altrimenti la campagna si rifà.

Per questo, dal 16 novembre 2026, il passo P0.6 ha un quinto sotto-passo _(scelta dell'utente)_:

| Sotto-passo | Contenuto |
| --- | --- |
| P0.6c | E-03 (cgroup e partizioni), E-04 (driver grafico) |
| P0.6d | E-05 (streaming nelle home cifrate), E-06 (rapporto tra E-core e P-core) |
| **P0.6e** | Chiusura dei punti aperti che toccano kernel, firmware o strumenti. Finisce con la configurazione dichiarata ferma. |
| P0.7 | Campagna di misura, sulla configurazione ferma |

## Da chiudere in P0.6c

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| — | Nessuno: il sotto-passo P0.6c è superato. | — | — |

## Da chiudere in P0.6d

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| — | Nessuno: il sotto-passo P0.6d è superato. A-31 e A-34 sono chiusi, in fondo; A-33 ha il suo rimedio ed è passato alla campagna. | — | — |

## Da chiudere in P0.6e, prima della campagna

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| — | Nessuno: il sotto-passo P0.6e è superato e la configurazione è ferma. A-35 e A-38 sono passati in «Più avanti». | — | — |

## Affidati alla campagna P0.7

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| A-16 | Buchi «hardware» rari, da 1 a 6 µs, senza causa nota, anche su core non live | P0.6b, parti 3 e 4c | Su 24 ore se ne vede la frequenza e se hanno un periodo. La campagna parte con la traccia armata sopra soglia. |
| A-30 | Con gli altri venti core a piena potenza (160 W) il ritardo massimo di risveglio sotto carico è salito da 4 a 7 µs, e a 11,6 µs con l'attesa attiva; due minuti per caso. **Primo lancio della campagna, 18 novembre 2026**: la miscela di carico del documento 0 porta il package a 120 W, non a 160 W, quindi questa condizione non è stata misurata; con quella miscela il massimo è 20,7 µs in un'ora | P0.6c, chiusura | Le 24 ore hanno una fase apposta, `power`: puro calcolo sui venti core non live, 10 minuti ogni 30, 8 ore in tutto, con il suo massimo per fase. Dirà se è una coda stabile o un caso. |
| A-36 | Il collegamento di rete è caduto per 3,5 secondi, 2,3 secondi dopo il primo caricamento dei moduli della macchina virtuale. Non si è ripetuto né al secondo caricamento, a macchina ferma, né al terzo, nelle condizioni del primo (misura in corso, package a 160 W). Una sola caduta in oltre un'ora | P0.6e, parte 1 | La campagna registra ogni caduta del collegamento. |
| A-33 | Un core live a riposo si risvegliava fino a 55 µs in ritardo quando gli altri P-core lavoravano a raffiche brevi e a frequenza bassa. Rimedio in opera dal 17 novembre 2026: P-core non live a frequenza fissa al massimo; misurato 6 µs nel caso che aveva fatto scoprire il problema. **Primo lancio della campagna, 18 novembre 2026**: con tre carichi a raffiche brevi diversi dalla cifratura il massimo per fase è 5–7 µs (un solo 12 µs), come a riposo; 7 fasi per tipo | P0.6d, parti 2 e 3; [ADR 0018](adr/0018-profili-di-avvio-e-regolazione.md), punto 17 | Il rimedio è provato con un solo carico a raffiche brevi, la cifratura. La campagna deve avere un carico a raffiche brevi suo sui P-core non live, con i core live a riposo tra un ciclo e l'altro. Se i risvegli lenti tornano, si riapre l'ADR 0009 (attesa attiva). |
| A-40 | Nell'ultima misura del rumore hardware (accettazione di P0.6e) il package è a 51 °C contro i 65 °C delle misure precedenti alla stessa potenza, e le letture sono 120 in dieci minuti invece di 53–55. A 160 W la temperatura è quella di sempre. **Dati grezzi guardati il 18 novembre 2026**: ogni core è più freddo di 7–12 °C, E-core compresi, quindi è il raffreddamento o la stanza (misura fatta di notte, dopo un riavvio), non il carico; le letture arrivano ogni 5,0 s invece di 7–12 s, e i core live sono al 75 % invece del 60–68 %, solo con il kernel 6.18.64, che corregge l'avviso a un core senza tick (ipotesi, non misura) | P0.6e, parte 3; [P0.7](fasi/fase-0/P0.7-campagna-di-misura.md), parte 1 | La campagna registra temperatura e potenza ogni 5 secondi per più di 24 ore con un carico che si ripete: se la temperatura segue l'ora del giorno, lo si vede. |
| A-17 | Regola per le misure di riferimento: cominciano almeno cinque minuti dopo l'accensione | P0.6b, parte 3 | Già negli strumenti come nota; la campagna la rispetta. |
| A-42 | Durante le prove di `tools/campaign.sh` la macchina virtuale emulata si è riavviata da sola, una volta, nella fase di riposo, intorno all'istante dell'arresto a mano. Nessun messaggio (con il profilo `285k` la console non va sulla seriale). Non riprodotto in due ripetizioni della stessa sequenza; nessun _watchdog_ in gioco | [P0.7](fasi/fase-0/P0.7-campagna-di-misura.md), parte 1 | Se il 285K si riavvia da solo durante la campagna, il messaggio è sullo schermo: va fotografato. Se la campagna finisce senza riavvii, il punto si chiude come fatto dell'emulazione. |

## Prima della Fase 1

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| A-06 | Con il _fair server_ acceso, in macchina virtuale, un processo interrotto dentro il kernel non è più ripartito e la macchina si è fermata. La 6.18.64 non ha correzioni in quell'area | P0.6b, parte 2 | Spostato qui da P0.6e per scelta dell'utente del 17 novembre 2026: va insieme ad A-18. Sui core live il _fair server_ è spento, e la configurazione certificata resta quella. |
| A-07 | `dawos-starve-test` esteso (terza variante, processo che non cede mai) non consegnato | P0.6b, parte 2 | Insieme ad A-06 e ad A-18: è la prova con cui si progetta e si verifica quella protezione. |
| A-18 | Protezione contro un thread real-time che non cede mai il core (rischio K-04) | P0.6b, parte 2; [ADR 0018](adr/0018-profili-di-avvio-e-regolazione.md) | Un progetto: ogni thread del motore cede a ogni ciclo, e un sorvegliante fuori dai core live interviene se non lo fa. Diventa un ADR. |
| A-41 | La campagna P0.7 non ha il carico di interrupt USB reale: il documento 0 voleva la Scarlett in riproduzione e registrazione continua «se disponibile», ma l'immagine ferma ha il driver e non i programmi ALSA | [P0.7](fasi/fase-0/P0.7-campagna-di-misura.md), parte 1 | All'inizio della Fase 1, con i programmi ALSA nell'immagine: una misura del ritardo di risveglio sui core live con la Scarlett che suona e registra, e gli interrupt USB sul dominio io. |
| A-19 | Domande D-10…D-15 e D-17…D-21, rinviate | Registro delle modifiche, parte 4 | Le ripropongo con le raccomandazioni aggiornate. |
| A-20 | D-23 (chiave delle immagini di rilascio), D-24 (blocco del kernel e vincolo sui moduli), D-25 (manutenzione periodica dei dischi) | Registro delle modifiche | Decisioni. D-24 tocca tre cose già fatte: lo spegnimento del _fair server_ (passa da `debugfs`), la lettura del registro del C1E, e i moduli del driver grafico. |
| A-21 | Sorveglianza del blocco del sistema (_watchdog_): il kernel non ha i rilevatori, e quello della scheda non è usato | P0.6a | Decidere se e quale. |
| A-32 | Nella prima misura di E-06 tre prove con quattro flussi sono uscite più lente dal 12 % al 77 %, per tutta la loro durata. Con i buffer sfalsati dentro la pagina non è più successo (60 misure); con i buffer allo stesso scostamento il costo sale del 3,5 %, e in una prova su dieci del 16 %. Il +77 % non si è ripetuto e non è spiegato | E-06 | Il motore sfalsa i buffer delle istanze (requisito per l'allocatore, Fase 1). Alla calibrazione del nodo nativo, ripetere il confronto tra le due disposizioni su più esecuzioni. |
| A-22 | Stati di riposo profondi assenti per tutti i core. Il manuale del firmware elenca per _CPU C-states_ anche il valore _Enabled_ (con _Package C State Limit_ a seguire), che sul firmware 2301 non compare | P0.6b, parte 4a; manuale della serie ROG STRIX Z890 (E25597) | Capire perché il valore manca (versione del firmware, o un'altra voce che lo nasconde). Nessuna urgenza: 3–4 W di package a macchina ferma. Va insieme alla scelta dei core che servono l'audio USB. |

## Più avanti

| Sigla | Punto | Da dove viene | Quando |
| --- | --- | --- | --- |
| A-29 | Su un kernel real-time l'unico driver per la GPU integrata del 285K è `xe`, sperimentale per questo processore in Linux 6.18. L'interfaccia della Fase 3 ne dipende | P0.6c, parte 2 | Seguire lo stato di `xe` per Arrow Lake e di `i915` sul real-time nelle prossime versioni del kernel; rivalutare alla scelta della LTS successiva. Il rischio è nel registro delle modifiche come K-16. |
| A-35 | Il kernel può ospitare una macchina virtuale (KVM, vhost, `tun`, traduzione degli indirizzi in nftables), come moduli che nulla carica all'avvio; il rumore di fondo non cambia, ma il caricamento manda 4200 interrupt brevi a ogni core live. L'ADR 033 a cui si riferiscono non è nel repository | P0.6e, parte 1 | Portare l'ADR 033 nel repository. Decidere quando si carica KVM: non durante l'audio; all'avvio, se la macchina virtuale parte a richiesta. Se si carica all'avvio, le misure brevi si rifanno con i moduli dentro. |
| A-38 | Il riavvio nelle impostazioni del firmware da sistema (`systemctl reboot --firmware-setup`) risponde «non supportato» sul 285K | P0.6e, parte 2 | Capire perché. Serve a chi deve cambiare una voce del firmware senza tastiera all'accensione. Nessuna urgenza. |
| A-39 | Undici dei 32 registri degli errori hardware non hanno il proprio interrupt (4, 5, 10, 11, 12, 13, 15, 28, 29, 30, 31): con il sondaggio periodico spento, un errore corretto lì non viene segnalato | P0.6e, parte 2; [ADR 0018](adr/0018-profili-di-avvio-e-regolazione.md), punto 12 | Decidere se interrogarli quando l'audio non gira (all'avvio, alla chiusura di una sessione). Insieme al sorvegliante del punto A-21. |
| A-37 | La configurazione del boot loader (`loader.conf`, sulla partizione EFI) non segue gli aggiornamenti: un valore nuovo vale solo per le installazioni nuove | P0.6e, parte 2 | Decidere se e come gli aggiornamenti la riscrivono, quando si definisce il percorso di aggiornamento del prodotto. |
| A-23 | Il carico grafico vero (disegno con la GPU) non è misurato. Con il solo cambio di pagina, ogni 227 secondi un interrupt di 0,3–0,7 µs raggiunge i core live (il kernel avvisa tutti i core quando libera memoria mappata); con un'interfaccia che disegna sarà più frequente | E-04; rischio K-16 | Fase 3, quando entra l'interfaccia: stessa misura con il carico vero; se serve, ridurre le mappature del driver o portare nel kernel il rinvio dell'avviso per i core isolati. |
| A-24 | Profilo delle ventole: oggi quello del firmware | P0.4b | Con il contenitore definitivo e le misure di rumore acustico. |
| A-25 | Revisione legale della licenza (GPLv3 con eccezione per i plugin) | [ADR 0010](adr/0010-licenza-gplv3-eccezione-plugin.md) | Prima di qualunque rilascio. |
| A-26 | Con una seconda macchina supportata: come si sceglie il profilo, e i numeri del riposo dei core | ADR 0018, [ADR 0009](adr/0009-riposo-dei-core-live.md) | Quando c'è la seconda macchina. |

## Chiusi

| Sigla | Punto | Chiuso |
| --- | --- | --- |
| A-11 | Il ritorno automatico all'immagine precedente con un profilo fissato era verificato nei sorgenti del boot loader, non provato sul 285K | 17 novembre 2026 (P0.6e, parte 3): provato. Un'immagine nuova fatta fallire apposta, con il profilo `285k` fissato: tre tentativi, poi il boot loader è tornato all'immagine di prima, con lo stesso profilo. |
| A-08 | `dawos-hwnoise`: l'ultima lettura di potenza poteva essere falsata (180–220 W in misure da 94 W di media) | 17 novembre 2026 (P0.6e, parti 2 e 3): l'ultima lettura è scartata; nella misura di accettazione il massimo è 96,1 W. |
| A-05 | Versione del kernel: restare su 6.18.63 o passare all'ultima 6.18.x | 17 novembre 2026 (P0.6e, parte 2): 6.18.64, per scelta dell'utente. Sul 285K: rumore zero sui core live, caso di A-33 a 5,9 µs. Il kernel non cambia più fino alla chiusura della fase. |
| A-09 | Opzioni del kernel previste e assenti: driver del TPM e audio USB | 17 novembre 2026 (P0.6e, parte 2): aggiunti, incorporati. Il TPM compare (`tpm0`, versione 2). L'audio USB si prova quando si collega un'interfaccia. |
| A-10 | Tempo di avvio: 14 secondi di firmware e 5 di menu | 17 novembre 2026 (P0.6e, parte 2): menu senza attesa, avvio da 24 a 20 secondi. Il firmware resta com'è, per scelta dell'utente: il tempo di avvio non è un requisito della Fase 0. |
| A-12 | Quanti registri degli errori hardware non hanno il proprio interrupt | 17 novembre 2026 (P0.6e, parte 2): 11 su 32. Il seguito è il punto A-39. |
| A-13 | Stato vero dei collegamenti PCIe | 17 novembre 2026 (P0.6e, parte 2): risparmio energetico spento su disco, scheda di rete e i loro ponti; acceso (`L1`) su due porte per periferiche esterne a cui non è collegato nulla. |
| A-14 | Avviso di systemd-resolved sul DNS non cifrato | 17 novembre 2026 (P0.6e, parte 2): `DNSOverTLS=no`, per scelta dell'utente; l'avviso non compare più. |
| A-15 | Chi manda gli interrupt di chiamata di funzione ai core live nei primi minuti | 17 novembre 2026 (P0.6e, parte 2): dopo i primi 4,5 secondi non ne arrivano più; i tre registrati vengono dal kernel che riscrive un'istruzione del proprio codice dopo il primo uso di una funzione. Regola ricavata: tutto ciò che fa riscrivere al kernel il proprio codice (moduli, tracce) va fatto prima che l'audio parta. |
| A-31 | La sessione di riferimento di R-04 a 96 kHz usa tra il 64 % e il 70 % del dominio anticipativo: fuori dalla soglia del 60 % | 17 novembre 2026 (P0.6d): scelta dell'utente, soglia distinta per il dominio anticipativo al 75 %; quella del dominio live resta il 60 %. Di riserva per la Fase 2: due E-core dal dominio di I/O, che E-05 ha misurato largo (2–11 %). ADR 0011 aggiornato. |
| A-34 | Su loop ogni `fdatasync` di un file che cresce costa il doppio (10–13 ms contro 5–6 ms): soglia di E-05 superata | 17 novembre 2026 (P0.6d, parte 2): su un file già scritto costa 4,9–6,1 ms. Scelta dell'utente: resta l'immagine su loop, con due vincoli scritti nell'ADR 0004 (dm-crypt senza code di lavoro; journal su segmenti già scritti). Il ripiego non si adotta. |
| A-03 | Sotto il profilo `285k` i core non live giravano piano e il package restava lontano dai 160 W | 17 novembre 2026 (P0.6c): la causa era la preferenza energetica `performance` data da `dawos-tune` ai soli core live; ora hanno quella degli altri. A pieno carico gli altri core passano da 2860 a 4454 MHz, il package da 122 a 160 W; i core live tengono i 4600 MHz, senza rumore e con lo stesso ritardo medio di risveglio. Con la macchina a piena potenza il ritardo massimo sotto carico sale da 4 a 7–12 µs: da seguire nella campagna P0.7. |
| A-02 | Driver grafico (esperimento E-04) | 17 novembre 2026 (P0.6c, parte 2): `xe`, l'unico che si compila su un kernel real-time; funziona sul 285K, zero rumore a schermo fermo. Il limite trovato con lo schermo che cambia è accettato dall'utente e passa ad A-23 e al rischio K-16. Il driver non si carica all'avvio. |
| A-01 | Il kernel non aveva il controllo della memoria per gruppo di processi (`CONFIG_MEMCG`) | 16 novembre 2026 (P0.6c, parte 2): aggiunto; la misura di rumore con il kernel nuovo passa e `dawos-cgroup-test` conferma il limite al gruppo del processo. |
| A-28 | Con il firmware 3402 le misure di rumore contavano a tratti il tick dello scheduler seguito da `rcuc` sui core live | 16 novembre 2026 (P0.6c, parte 2): è un effetto del thread del tracciatore, che vive nel kernel. Con un ciclo in spazio utente al suo posto (`dawos-osnoise --user`), in dieci minuti il kernel non è mai entrato sui core live. Perché compaia più spesso con il firmware nuovo non è spiegato; non riguarda il motore. |
| A-27 | Firmware del 285K dalla 2301 alla 3402, con l'Intel ME | 16 novembre 2026 (P0.6c, parte 0): aggiornato; nessun SMI; frequenza, risveglio e limite di potenza rimisurati; 4,6 GHz confermati. Ha lasciato A-28 e ha aggravato A-03. |
| A-04 | Il ramo d'errore della verifica del C1E non era provato sulla macchina vera | 16 novembre 2026: al primo avvio dopo l'aggiornamento del firmware, con i valori di fabbrica, `make remote-test` è fallito con `tune=degraded` (riferito dall'utente; l'uscita non è stata conservata). |
