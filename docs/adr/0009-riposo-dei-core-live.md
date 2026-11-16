# ADR 0009 — Stato di riposo dei core live

- **Stato**: in sospeso; si decide sui numeri del passo P0.6
- **Data**: 13 novembre 2026
- **Origine**: D-09 e K-04 del documento 0

## Contesto

Il master prompt prescrive il solo stato di polling sui core live, tramite PM QoS. Con l'attesa attiva dei worker, questo tiene 4 P-core sempre al massimo consumo: decine di watt costanti e ventole che girano in uno studio di registrazione. D'altra parte ogni stato di riposo più profondo aggiunge latenza al risveglio.

## Decisione

**Nessuna decisione ora.** Il passo P0.6 misura i casi sotto e la scelta si fa sui risultati.

## Cosa si misura

Quattro configurazioni sui core live, ciascuna con una prova `timerlat` di 30 minuti sotto carico:

| Caso | Stato di riposo ammesso | PM QoS |
| --- | --- | --- |
| A | solo polling | sempre attivo |
| B | fino a C1 | sempre attivo |
| C | fino a C1E | sempre attivo |
| D | predefinito del sistema | nessuno |

Per ciascuna: latenza massima e 99,999-esimo percentile, temperatura dei core, potenza del package da `turbostat`, residenza negli stati di riposo, e, se possibile, la velocità delle ventole.

## Aggiornamento del 16 novembre 2026: i casi sul 285K

I quattro casi qui sopra presuppongono che il kernel abbia più stati di riposo tra cui scegliere, e che un limite dato core per core (PM QoS) ne escluda alcuni. Sul 285K non è così _(verificato nel sotto-passo P0.6b, parte 4a)_:

- il firmware non dichiara al sistema alcuno stato di riposo: il kernel ferma un core con una sola istruzione, e il limite per core non ha nulla su cui agire;
- che cosa succeda a un core fermo lo decide una voce del firmware, _Enhanced C-states_, uguale per tutti i core: accesa (è il valore di fabbrica) il core scende a C1E, spenta resta in C1;
- stati più profondi non ci sono, per nessun core.

I casi diventano:

| Caso | Sul 285K |
| --- | --- |
| A, solo polling | Attesa attiva: un processo tiene sveglio ogni core live. È una scelta del motore, non del sistema. |
| B, fino a C1 | _Enhanced C-states: Disabled_ nel firmware. Vale per tutta la macchina. |
| C, fino a C1E | _Enhanced C-states_ acceso. Vale per tutta la macchina. |
| D, predefinito | Coincide con C. |

Si misurano con `dawos-wakeup`: ritardo di risveglio con e senza attesa attiva, a macchina tranquilla e sotto carico, più potenza e temperatura a riposo, una volta per ciascuna impostazione del firmware. Ogni caso dura due minuti, che bastano per media e minimo; massimo e 99,999-esimo percentile della configurazione scelta si confermano con una misura lunga prima di chiudere questo ADR. La residenza negli stati di riposo non si può leggere (senza driver il kernel non la conta); al suo posto c'è la quota di tempo in cui i core sono stati fermi.

Il criterio di scelta non cambia. Cambia una conseguenza: tra B e C non si sceglie per i soli core live ma per l'intera macchina, quindi nel conto entrano anche consumo e temperatura a riposo di tutti gli altri core.

## Criterio di scelta

Si adotta la configurazione più parca che rispetti entrambe le condizioni:

- latenza massima entro la soglia di 50 µs con almeno 20 µs di margine;
- peggioramento rispetto al caso A non superiore a 5 µs sulla latenza massima.

Se più configurazioni le rispettano, si tiene la più parca a motore fermo e il polling solo a motore armato.

## Aspettativa

C1 a motore fermo e polling a motore armato _(stima; l'uscita da C1 sui P-core dovrebbe costare 1–2 µs)_.

## Conseguenze della sospensione

Fino alla decisione, lo script di regolazione del passo P0.6 prende lo stato di riposo da un parametro, e le prove della campagna P0.7 dichiarano con quale valore sono state fatte.

## Quando si chiude

Alla fine del passo P0.6, con un aggiornamento di questo ADR che riporta i numeri.
