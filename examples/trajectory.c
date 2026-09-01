/* A satellite track, and a bug: the tail is freed and then read. */
#include <stdio.h>
#include <stdlib.h>

typedef struct Fix { int alt; struct Fix *next; } Fix;

static Fix *push(Fix *tail, int alt) {
    Fix *fix = malloc(sizeof(Fix));
    fix->alt = alt;
    fix->next = NULL;
    if (tail) tail->next = fix;
    return fix;
}

int main(void) {
    Fix *head = push(NULL, 420);
    Fix *mid  = push(head, 415);
    Fix *tail = push(mid, 408);

    free(tail);                     /* mid->next now dangles */
    printf("last altitude: %d\n", mid->next->alt);

    free(mid);
    free(head);
    return 0;
}
