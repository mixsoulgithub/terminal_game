#include "object.hpp"
#include <stdexcept>
//静态成员变量必须在类外初始化.

Outlook Object::default_outlook=std::make_tuple(std::string(" "),1);

Object::Object(Outlook outlook)
{
    default_outlook=outlook;
}

const std::vector<Body>& Object::get_body() const
{
    return body;
}

const Body& Object::get_body(int i) const
{
    if(i<0 || i>=body.size()) throw std::out_of_range("Index out of range in Object::get_body");
    return body[i];
}

int Object::set_body(int i,Body& body_part)
{
    if(i<0 || i>=body.size()) return -1;
    body[i]=std::move(body_part);
    return 0;
}

int Object::delete_body(int i)
{
    if(i<0 || i>=body.size()) return -1;
    body.erase(body.begin()+i);
    return 0;
}

int Object::insert_body(int i, Body& body_part)
{
    if(i<0 || i>body.size()) return -1;
    body.emplace(body.begin()+i,std::move(body_part));
    return 0;
}