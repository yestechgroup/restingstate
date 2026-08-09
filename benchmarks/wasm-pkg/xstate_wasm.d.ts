/* tslint:disable */
/* eslint-disable */

export class JsMachine {
    free(): void;
    [Symbol.dispose](): void;
    constructor(machine_json: string);
    send(event_json: string): string | undefined;
    snapshot(): string;
}

export function init(): void;
