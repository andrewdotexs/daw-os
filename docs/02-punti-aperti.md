# Punti aperti

- **Che cos'è**: l'elenco unico delle cose rimaste in sospeso lungo i passi. Ogni punto dice da dove viene, che cosa serve per chiuderlo e in quale passo si chiude.
- **Chi lo aggiorna**: si aggiorna alla chiusura di ogni passo. Un punto chiuso passa nell'ultima tabella con la data e il modo; non si cancella.
- **Che cosa non contiene**: le domande di progetto (D-xx) e i rischi (K-xx) hanno già il loro posto nel [registro delle modifiche](01-registro-modifiche-v1.md); qui compaiono solo quando c'è un lavoro da fare.
- **Stato**: aggiornato il 16 novembre 2026, alla chiusura del sotto-passo P0.6b e all'avvio di P0.6c; lo stesso giorno aggiunto A-27.

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
| A-01 | Il kernel non ha il controllo della memoria per gruppo di processi (`CONFIG_MEMCG`): `MemoryMax=` di systemd oggi non ha effetto | Trovato il 16 novembre 2026 provando E-03 | Aggiungerlo nella stessa ricostruzione del kernel che porta `i915` (parte 2), e ripetere la misura di rumore: il controllo della memoria ha lavoro periodico suo. |
| A-27 | Firmware del 285K: dalla 2301 (ottobre 2025) alla 3402 (settembre 2026), con l'Intel ME | Visto dall'utente il 16 novembre 2026, che ha scelto di aggiornare subito | Parte 0 di P0.6c: aggiornamento, impostazioni rimesse, stato della macchina prima e dopo, misure rifatte. Chiude anche A-04 e dà una risposta su A-22. |
| A-02 | Driver grafico: `i915` come modulo; `xe` solo se `i915` fa rumore | E-04; scelta dell'utente del 16 novembre 2026 | Parte 2 di P0.6c. |

## Da chiudere in P0.6d

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| A-03 | Sotto carico i core non live girano più piano del previsto e il package resta sotto i 160 W; con i core live occupati scende ancora di 5–10 W | P0.6b, parti 3 e 4 | Si misura con E-06, che guarda proprio la resa dei core. Due sospetti, entrambi voci del firmware: _HwP Autonomous EPP Grouping_ (acceso: il manuale dice che con questa voce il processore chiede lo stesso valore per tutti i core con la stessa preferenza energetica) e i limiti di corrente (_CPU Core/Cache Current Limit Max._, _IA CEP_). |
| A-04 | Il ramo d'errore della verifica del C1E non è provato sulla macchina vera | P0.6b, parte 4c | Alla prima visita nel firmware, che sarà quella di A-03: rimettere _Enhanced C-states_ su _Auto_, vedere `tune=degraded`, rimetterlo su _Disabled_. |

## Da chiudere in P0.6e, prima della campagna

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| A-05 | Versione del kernel: restare su 6.18.63 o passare all'ultima 6.18.x prima di certificare | [ADR 0007](adr/0007-versioni-buildroot-e-kernel.md) | Guardare che cosa è uscito e decidere insieme. Se si cambia, tutte le prove automatiche e le misure brevi si rifanno. |
| A-06 | Con il _fair server_ acceso, in macchina virtuale, un processo interrotto dentro il kernel non è più ripartito e la macchina si è fermata | P0.6b, parte 2 | Cercare la correzione nelle versioni successive del kernel (va insieme ad A-05) e ripetere la prova estesa finché passa. Serve prima della Fase 1, ma dipende dalla versione del kernel: per questo sta qui. |
| A-07 | `dawos-starve-test` esteso (terza variante, processo che non cede mai) non consegnato | P0.6b, parte 2 | Insieme ad A-06. |
| A-08 | `dawos-hwnoise`: l'ultima lettura di potenza può essere falsata (219,6 W in una misura da 94 W di media) | P0.6b, parte 4c | Scartare l'ultima lettura, o leggere la potenza dai file del kernel come fanno gli altri strumenti. |
| A-09 | Opzioni del kernel previste dal documento 0 e non ancora presenti: driver del TPM (`TCG_TPM`, `TCG_CRB`, `TCG_TIS`) | P0.3, tabella delle opzioni; visto in P0.5 | Aggiungerle, e controllare la tabella intera contro la configurazione vera. |
| A-10 | Tempo di avvio: 14 secondi di firmware e 5 di menu | P0.5f | Voci del firmware (_Fast Boot_, _POST Delay Time_, logo) e tempo di attesa del menu. Tocca il firmware: per questo prima della campagna. |
| A-11 | Il ritorno automatico all'immagine precedente con un profilo fissato è verificato nei sorgenti del boot loader, non provato sul 285K | [ADR 0018](adr/0018-profili-di-avvio-e-regolazione.md) | Una prova: un aggiornamento che fallisce apposta, con il profilo `285k` fissato. |
| A-12 | Quanti registri degli errori hardware del 285K non hanno il proprio interrupt (per quelli, spento il sondaggio, gli errori corretti non vengono più segnalati) | [ADR 0018](adr/0018-profili-di-avvio-e-regolazione.md) | Una lettura dal sistema. |
| A-13 | Risparmio energetico dei collegamenti PCIe (ASPM): gestito dal firmware, spento nelle sue voci, ma lo stato vero dei collegamenti non è stato letto | P0.4b | Una lettura con `lspci`. |
| A-14 | Avviso di systemd-resolved sul passaggio a DNS non cifrato | P0.5 | Decidere l'impostazione e scriverla. |
| A-15 | Gli interrupt di chiamata di funzione sui core live nei primi minuti dopo l'accensione (dieci per core): chi li manda non è noto | P0.6b, parte 3 | Una traccia nei primi cinque minuti. Oggi la regola è misurare dopo cinque minuti; per il motore bisogna sapere se è lavoro dell'avvio o di qualcosa che può tornare. |

## Affidati alla campagna P0.7

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| A-16 | Buchi «hardware» rari, da 1 a 6 µs, senza causa nota, anche su core non live | P0.6b, parti 3 e 4c | Su 24 ore se ne vede la frequenza e se hanno un periodo. La campagna parte con la traccia armata sopra soglia. |
| A-17 | Regola per le misure di riferimento: cominciano almeno cinque minuti dopo l'accensione | P0.6b, parte 3 | Già negli strumenti come nota; la campagna la rispetta. |

## Prima della Fase 1

| Sigla | Punto | Da dove viene | Che cosa serve |
| --- | --- | --- | --- |
| A-18 | Protezione contro un thread real-time che non cede mai il core (rischio K-04) | P0.6b, parte 2; [ADR 0018](adr/0018-profili-di-avvio-e-regolazione.md) | Un progetto: ogni thread del motore cede a ogni ciclo, e un sorvegliante fuori dai core live interviene se non lo fa. Diventa un ADR. |
| A-19 | Domande D-10…D-15 e D-17…D-21, rinviate | Registro delle modifiche, parte 4 | Le ripropongo con le raccomandazioni aggiornate. |
| A-20 | D-23 (chiave delle immagini di rilascio), D-24 (blocco del kernel e vincolo sui moduli), D-25 (manutenzione periodica dei dischi) | Registro delle modifiche | Decisioni. D-24 tocca tre cose già fatte: lo spegnimento del _fair server_ (passa da `debugfs`), la lettura del registro del C1E, e i moduli del driver grafico. |
| A-21 | Sorveglianza del blocco del sistema (_watchdog_): il kernel non ha i rilevatori, e quello della scheda non è usato | P0.6a | Decidere se e quale. |
| A-22 | Stati di riposo profondi assenti per tutti i core. Il manuale del firmware elenca per _CPU C-states_ anche il valore _Enabled_ (con _Package C State Limit_ a seguire), che sul firmware 2301 non compare | P0.6b, parte 4a; manuale della serie ROG STRIX Z890 (E25597) | Capire perché il valore manca (versione del firmware, o un'altra voce che lo nasconde). Nessuna urgenza: 3–4 W di package a macchina ferma. Va insieme alla scelta dei core che servono l'audio USB. |

## Più avanti

| Sigla | Punto | Da dove viene | Quando |
| --- | --- | --- | --- |
| A-23 | Il carico grafico vero (disegno con la GPU) non è misurato: in P0.6c si misura solo lo schermo attivo | E-04 | Fase 3, quando entra l'interfaccia. |
| A-24 | Profilo delle ventole: oggi quello del firmware | P0.4b | Con il contenitore definitivo e le misure di rumore acustico. |
| A-25 | Revisione legale della licenza (GPLv3 con eccezione per i plugin) | [ADR 0010](adr/0010-licenza-gplv3-eccezione-plugin.md) | Prima di qualunque rilascio. |
| A-26 | Con una seconda macchina supportata: come si sceglie il profilo, e i numeri del riposo dei core | ADR 0018, [ADR 0009](adr/0009-riposo-dei-core-live.md) | Quando c'è la seconda macchina. |

## Chiusi

| Sigla | Punto | Chiuso |
| --- | --- | --- |
| — | Ancora nessuno: il registro nasce il 16 novembre 2026. | — |
