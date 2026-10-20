#include "../OwnerRelevancy.h"
#include <cassert>
#include <iostream>
struct Actor { Actor* owner = nullptr; };
int main() {
    Actor controller, otherController, pawn{&controller}, equipment{&pawn}, otherPawn{&otherController};
    auto owner=[](Actor* a){ return a->owner; };
    assert(IsConnectionOwner(&controller,&controller,&pawn,owner));
    assert(IsConnectionOwner(&pawn,&controller,&pawn,owner));
    assert(IsConnectionOwner(&equipment,&controller,&pawn,owner));
    assert(!IsConnectionOwner(&otherPawn,&controller,&pawn,owner));
    assert(!IsConnectionOwner(&otherController,&controller,&pawn,owner));
    assert(!IsConnectionOwner(static_cast<Actor*>(nullptr),&controller,&pawn,owner));
    Actor a,b;a.owner=&b;b.owner=&a;
    assert(!IsConnectionOwner(&a,&controller,&pawn,owner));
    a.owner=&a;assert(!IsConnectionOwner(&a,&controller,&pawn,owner));
    std::cout << "owner-only replication isolation passed\n";
}
