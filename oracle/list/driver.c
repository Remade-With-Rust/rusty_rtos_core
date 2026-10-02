/*
 * The list differential, the C side (hardening gate H-29 for rusty_rtos_core).
 *
 * FreeRTOS-Kernel V11.3.1's own list.c -- the pinned oracle, unmodified --
 * driven by a 32-bit xorshift through a random script of vListInsert,
 * vListInsertEnd, uxListRemove and listGET_OWNER_OF_NEXT_ENTRY over a few
 * lists and a pool of items. After every step it prints the touched list:
 * its items in order with their values, its length, and where its
 * round-robin index (pxIndex) points.
 *
 * `crates/rusty_rtos_core/tests/list_differential.rs` runs the SAME script
 * against `ListsOf` and must print the same lines. Values are drawn from a
 * small range so ties are common (vListInsert puts an item AFTER its equals)
 * and portMAX_DELAY appears (the end-marker shortcut).
 *
 * Build and run (WSL, the oracle fetched by `kairos oracle fetch`):
 *     cd oracle/list && sh run.sh
 */
#include <stdint.h>
#include <stdio.h>

#include "FreeRTOS.h"
#include "list.h"

#define STEPS 50000
#define LISTS 4
#define ITEMS 24

static uint32_t rng = 0x9e3779b9u;
static uint32_t next( void )
{
    rng ^= rng << 13;
    rng ^= rng >> 17;
    rng ^= rng << 5;
    return rng;
}

static List_t lists[ LISTS ];
static ListItem_t items[ ITEMS ];

static int index_of( const ListItem_t * p )
{
    return ( int ) ( p - items );
}

static void print_list( unsigned step, const char * op, long r, int l )
{
    const List_t * pl = &lists[ l ];
    const ListItem_t * p = listGET_HEAD_ENTRY( pl );
    const ListItem_t * end = listGET_END_MARKER( pl );

    printf( "%u %s r=%ld L%d len=%lu [", step, op, r, l, ( unsigned long ) listCURRENT_LIST_LENGTH( pl ) );
    while( p != end )
    {
        printf( " %d:%lu", index_of( p ), ( unsigned long ) listGET_LIST_ITEM_VALUE( p ) );
        p = listGET_NEXT( p );
    }
    if( pl->pxIndex == ( ListItem_t * ) end )
    {
        printf( " ] idx=end\n" );
    }
    else
    {
        printf( " ] idx=%d\n", index_of( pl->pxIndex ) );
    }
}

int main( void )
{
    char op[ 48 ];

    for( int l = 0; l < LISTS; l++ )
    {
        vListInitialise( &lists[ l ] );
    }
    for( int i = 0; i < ITEMS; i++ )
    {
        vListInitialiseItem( &items[ i ] );
        listSET_LIST_ITEM_OWNER( &items[ i ], &items[ i ] );
    }

    for( unsigned step = 1; step <= STEPS; step++ )
    {
        unsigned kind = next() % 100;
        int l = ( int ) ( next() % LISTS );
        int i = ( int ) ( next() % ITEMS );
        uint32_t arg = next();
        ListItem_t * it = &items[ i ];
        List_t * owner = listLIST_ITEM_CONTAINER( it );
        long r = 0;

        if( kind < 35 )
        {
            if( owner != NULL )
            {
                continue;
            }
            /* Small values make ties; one in sixteen is portMAX_DELAY. */
            TickType_t v = ( arg % 16 == 0 ) ? portMAX_DELAY : ( TickType_t ) ( arg % 6 );
            listSET_LIST_ITEM_VALUE( it, v );
            vListInsert( &lists[ l ], it );
            snprintf( op, sizeof op, "insert %d %lu", i, ( unsigned long ) v );
            print_list( step, op, r, l );
        }
        else if( kind < 60 )
        {
            if( owner != NULL )
            {
                continue;
            }
            vListInsertEnd( &lists[ l ], it );
            snprintf( op, sizeof op, "insert_end %d", i );
            print_list( step, op, r, l );
        }
        else if( kind < 85 )
        {
            if( owner == NULL )
            {
                continue;
            }
            int from = ( int ) ( owner - lists );
            r = ( long ) uxListRemove( it );
            snprintf( op, sizeof op, "remove %d", i );
            print_list( step, op, r, from );
        }
        else
        {
            if( listLIST_IS_EMPTY( &lists[ l ] ) )
            {
                continue;
            }
            ListItem_t * got;
            listGET_OWNER_OF_NEXT_ENTRY( got, &lists[ l ] );
            r = index_of( got );
            snprintf( op, sizeof op, "next" );
            print_list( step, op, r, l );
        }
    }
    return 0;
}
