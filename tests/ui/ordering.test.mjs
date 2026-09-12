import {test} from 'node:test';
import {ok} from 'node:assert/strict';
import {orderingChecks} from './ordering.js';

await orderingChecks(test, ok);
